// three.js の経路描画。早送りは破線、切削は実線で工具ごとに色。
// 1 画面と 4 分割（XY・3D・XZ・YZ）。WebGL のコンテキストは 1 つで、scissor と viewport で区切って描く。
// 操作：左ドラッグで移動、ホイールで拡大・縮小、右ドラッグで 3D 回転、クリックで経路を選ぶ、
//       中ボタンのダブルクリックでそのビューをフィット、4 分割で左ダブルクリックするとそのビューを最大化（もう一度で戻す）。
//       4 分割の境界（縦・横・中央の十字）はドラッグで大きさを変え、ダブルクリックで均等に戻す。
// 座標は表示中心からの相対で f32 に詰める（03_GUI方針 D33）。
import * as THREE from "three";
import { LineMaterial } from "three/examples/jsm/lines/LineMaterial.js";
import { LineSegments2 } from "three/examples/jsm/lines/LineSegments2.js";
import { LineSegmentsGeometry } from "three/examples/jsm/lines/LineSegmentsGeometry.js";
import type { Segment } from "./types";

export type ViewName = "xy" | "yz" | "xz" | "3d";
export type Layout = "single" | "quad";

const TOOL_COUNT = 7; // style.css の --tool-0〜--tool-6
/** 4 分割の境界の掴める太さ（px）と、各ビューが残す最小の大きさ（px） */
const BAR = 8, MIN_PANE = 80;

/** 描画の色。style.css の CSS 変数から読む（テーマの切り替えで読み直す）。 */
interface Palette { bg: string; rapid: string; hl: string; dim: string; tool: string; toolEdge: string; ghost: string; tools: string[]; axes: [string, string, string] }
function readPalette(): Palette {
  const css = getComputedStyle(document.documentElement);
  const v = (name: string) => css.getPropertyValue(name).trim() || "#888888";
  return {
    bg: v("--view-bg"),
    rapid: v("--view-rapid"),
    hl: v("--view-hl"),
    dim: v("--view-dim"),
    tool: v("--view-tool"),
    toolEdge: v("--view-tool-edge"),
    ghost: v("--view-ghost"),
    tools: Array.from({ length: TOOL_COUNT }, (_, i) => v(`--tool-${i}`)),
    axes: [v("--axis-x"), v("--axis-y"), v("--axis-z")],
  };
}
const VIEWS: Record<ViewName, { az: number; el: number; label: string; axes: string }> = {
  xy: { az: -90, el: 90, label: "XY", axes: "XY" },
  yz: { az: 0, el: 0, label: "YZ", axes: "YZ" },
  xz: { az: -90, el: 0, label: "XZ", axes: "XZ" },
  "3d": { az: -55, el: 35, label: "3D", axes: "" },
};
/** 4 分割の並び：左上・右上・左下・右下 */
const QUAD: ViewName[] = ["xy", "3d", "xz", "yz"];

/** 凡例などで使う工具の色（CSS 変数）。 */
export const toolColor = (tool: number) => `var(--tool-${tool % TOOL_COUNT})`;

interface Rect { x: number; y: number; w: number; h: number }

/** 1 つのビュー：視点・注視点・倍率は独立。 */
class Port {
  camera = new THREE.OrthographicCamera(-1, 1, 1, -1, -1e7, 1e7);
  target = new THREE.Vector3(); // 注視点（表示中心からの相対）
  az: number;
  el: number;
  scale = 1; // 1 mm あたりの画素数
  rect: Rect = { x: 0, y: 0, w: 1, h: 1 };
  frame: HTMLElement;

  constructor(readonly name: ViewName, host: HTMLElement) {
    ({ az: this.az, el: this.el } = VIEWS[name]);
    this.frame = document.createElement("div");
    this.frame.className = "view-frame";
    this.frame.innerHTML = `<span>${VIEWS[name].label}</span>`;
    host.append(this.frame);
  }

  get top() { return Math.abs(this.el) > 89.99; }

  update() {
    const { w, h } = this.rect;
    const az = THREE.MathUtils.degToRad(this.az), el = THREE.MathUtils.degToRad(this.el);
    const dir = new THREE.Vector3(Math.cos(el) * Math.cos(az), Math.cos(el) * Math.sin(az), Math.sin(el));
    // 真上・真下から見るときは +Y を画面の上に
    if (this.top) this.camera.up.set(0, Math.sign(this.el), 0);
    else this.camera.up.set(0, 0, 1);
    this.camera.position.copy(this.target).addScaledVector(dir, 1000);
    this.camera.lookAt(this.target);
    this.camera.left = -w / 2 / this.scale;
    this.camera.right = w / 2 / this.scale;
    this.camera.top = h / 2 / this.scale;
    this.camera.bottom = -h / 2 / this.scale;
    this.camera.updateProjectionMatrix();
    this.camera.updateMatrixWorld();
  }

  /** ビュー内の画素 → 表示座標（注視点を通る面の上）。 */
  screenToWorld(x: number, y: number) {
    return new THREE.Vector3((x / this.rect.w) * 2 - 1, -(y / this.rect.h) * 2 + 1, 0).unproject(this.camera);
  }
}

export class PathViewer {
  private renderer: THREE.WebGLRenderer;
  private scene = new THREE.Scene();
  private pathGroup = new THREE.Group();
  private ghostGroup = new THREE.Group();
  private showGhost = true;
  private ghostMaterial = new THREE.LineBasicMaterial({ depthWrite: false });
  private ghostRapid = new THREE.LineDashedMaterial({ depthWrite: false });
  private codeRows: ReadonlySet<number> | null = null;
  private highlightedLine = -1;
  private progress = { k: 0, frac: 0 };
  private hlGroup = new THREE.Group();
  /** 強調（行の選択・使用コードの区間）は画面上の太さで描く。深さに関係なく手前に */
  private hlMat = new LineMaterial({ linewidth: 3, depthTest: false, depthWrite: false });
  /** 使用コード一覧で選んだ区間の太線と、その線分の番号（昇順）。再生では描く本数だけを切り替える */
  private sel = new LineSegments2(new LineSegmentsGeometry(), this.hlMat);
  private selIdx = new Int32Array(0);
  private axes = new THREE.AxesHelper(1);
  private dashed: THREE.LineDashedMaterial[] = [];
  private segs: Segment[] = [];
  /** 種類・工具ごとの線の束と、その束に入れた線分の番号（昇順）。再生では描く本数だけを切り替える */
  private bundles: { line: THREE.LineSegments<THREE.BufferGeometry, THREE.LineBasicMaterial>; idx: Int32Array; tool: number }[] = [];
  private partial = new THREE.LineSegments(new THREE.BufferGeometry(), new THREE.LineBasicMaterial({ color: 0xffffff }));
  private tool = makeTool();
  private pal = readPalette();
  private center = new THREE.Vector3(); // 表示中心（ワーク座標）
  private ports: Record<ViewName, Port>;
  private layout: Layout = "single";
  private active: ViewName = "xy";
  /** 4 分割から 1 つのビューを最大化している */
  private maximized = false;
  /** 4 分割の境界の位置（幅・高さに対する割合） */
  private split = { x: 0.5, y: 0.5 };
  /** 4 分割の境界：縦・横・中央の十字 */
  private bars: Record<"v" | "h" | "c", HTMLElement>;
  private frame = 0;
  /** 利用者の操作で視点・倍率・分け方が変わった（設定の保存に使う） */
  onChange: () => void = () => {};
  onContextMenu: (e: MouseEvent) => void = () => {};
  /** タッチの操作（Web 版）：1 本指で回転（2D は移動）、2 本指のピンチで拡大・縮小、2 本指のドラッグで移動、ダブルタップでフィット */
  touchGestures = false;

  constructor(
    private host: HTMLElement,
    private onPick: (srcLine: number) => void,
    private onHover: (text: string) => void,
    private onLayout: (layout: Layout, active: ViewName, maximized: boolean) => void,
  ) {
    this.renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
    this.renderer.setPixelRatio(window.devicePixelRatio);
    this.renderer.setScissorTest(true);
    host.prepend(this.renderer.domElement);
    this.ports = Object.fromEntries(QUAD.map((n) => [n, new Port(n, host)])) as Record<ViewName, Port>;
    this.bars = this.makeSplitBars();
    this.applyPalette();
    this.partial.geometry.setAttribute("position", new THREE.Float32BufferAttribute(new Float32Array(6), 3));
    this.partial.frustumCulled = false;
    this.tool.visible = false;
    this.sel.renderOrder = 2;
    this.sel.visible = false;
    this.scene.add(this.ghostGroup, this.pathGroup, this.sel, this.hlGroup, this.axes, this.partial, this.tool);
    new ResizeObserver(() => this.resize()).observe(host);
    this.bindMouse();
    this.resize();
  }

  /** 経路を入れ替える。keepView なら視点と倍率を保つ（編集中の再実行）。 */
  setPath(segs: Segment[], keepView = false) {
    const oldCenter = this.center.clone();
    this.segs = segs;
    for (const b of this.bundles) { b.line.geometry.dispose(); b.line.material.dispose(); }
    for (const child of this.ghostGroup.children as THREE.LineSegments[]) child.geometry.dispose();
    this.ghostGroup.clear();
    this.pathGroup.clear();
    this.clearLineHighlight();
    this.dashed = [];
    this.bundles = [];
    this.clearProgress();
    const box = new THREE.Box3();
    for (const s of segs) {
      box.expandByPoint(new THREE.Vector3(...s.from));
      box.expandByPoint(new THREE.Vector3(...s.to));
    }
    if (!box.isEmpty()) box.getCenter(this.center);
    else this.center.set(0, 0, 0);

    // 種類・工具ごとに 1 つの LineSegments にまとめる
    const groups = new Map<string, { pos: number[]; idx: number[] }>();
    segs.forEach((s, i) => {
      const key = s.kind === "Rapid" ? "r" : `c${s.tool_index}`;
      if (!groups.has(key)) groups.set(key, { pos: [], idx: [] });
      const g = groups.get(key)!;
      g.pos.push(...this.rel(s.from), ...this.rel(s.to));
      g.idx.push(i);
    });
    for (const [key, g] of groups) {
      const geo = new THREE.BufferGeometry();
      geo.setAttribute("position", new THREE.Float32BufferAttribute(g.pos, 3));
      const tool = key === "r" ? -1 : Number(key.slice(1));
      let line: THREE.LineSegments<THREE.BufferGeometry, THREE.LineBasicMaterial>;
      if (key === "r") {
        const mat = new THREE.LineDashedMaterial({ vertexColors: true, dashSize: 1, gapSize: 1 });
        this.dashed.push(mat);
        line = new THREE.LineSegments(geo, mat).computeLineDistances();
      } else {
        line = new THREE.LineSegments(geo, new THREE.LineBasicMaterial({ vertexColors: true }));
      }
      geo.setAttribute("color", new THREE.BufferAttribute(new Float32Array(g.pos.length), 3));
      const ghostGeo = new THREE.BufferGeometry();
      ghostGeo.setAttribute("position", geo.getAttribute("position"));
      if (key === "r") {
        ghostGeo.setAttribute("lineDistance", geo.getAttribute("lineDistance"));
        this.dashed.push(this.ghostRapid);
      }
      const ghost = new THREE.LineSegments(ghostGeo, key === "r" ? this.ghostRapid : this.ghostMaterial);
      ghost.renderOrder = -1;
      this.ghostGroup.add(ghost);
      this.pathGroup.add(line);
      this.bundles.push({ line, idx: Int32Array.from(g.idx), tool });
    }
    this.colorPaths();
    this.axes.position.copy(this.center).negate();
    const size = box.isEmpty() ? 10 : Math.max(...box.getSize(new THREE.Vector3()).toArray(), 1);
    this.axes.scale.setScalar(size * 0.15);
    if (keepView) {
      for (const p of this.allPorts()) {
        p.target.add(oldCenter).sub(this.center);
        p.update();
      }
      this.render();
    } else for (const p of this.allPorts()) this.fitPort(p);
  }

  /** 指定行の経路を強調。 */
  highlightLine(srcLine: number) {
    this.clearLineHighlight();
    this.highlightedLine = srcLine;
    const arr: number[] = [];
    if (!this.codeRows) {
      for (const s of this.segs) if (s.src_line === srcLine) arr.push(...this.rel(s.from), ...this.rel(s.to));
    }
    if (arr.length) {
      const fat = new LineSegments2(new LineSegmentsGeometry().setPositions(arr), this.hlMat);
      fat.renderOrder = 3;
      this.hlGroup.add(fat);
      // 長さの無い線分だけの行は、太線が見えないので点で示す
      let zero = true;
      for (let i = 0; i < arr.length && zero; i += 6) zero = arr[i] === arr[i + 3] && arr[i + 1] === arr[i + 4] && arr[i + 2] === arr[i + 5];
      if (zero) {
        const geo = new THREE.BufferGeometry();
        geo.setAttribute("position", new THREE.Float32BufferAttribute(arr, 3));
        const dots = new THREE.Points(geo, new THREE.PointsMaterial({ color: this.pal.hl, size: 6, sizeAttenuation: false, depthTest: false }));
        dots.renderOrder = 3;
        this.hlGroup.add(dots);
      }
    }
    this.render();
  }

  private clearLineHighlight() {
    for (const child of this.hlGroup.children as (LineSegments2 | THREE.Points)[]) {
      child.geometry.dispose();
      if (child.material !== this.hlMat) (child.material as THREE.Material).dispose();
    }
    this.hlGroup.clear();
  }

  setCodeRows(rows: ReadonlySet<number> | null) {
    if (!rows && !this.codeRows) return;
    this.codeRows = rows;
    this.colorPaths();
    this.highlightLine(this.highlightedLine);
    this.setProgress(this.progress.k, this.progress.frac);
  }

  /** 完成形の色（空はテーマの既定）と薄さ（0〜1） */
  private ghostStyle = { color: "", fade: 0 };
  setGhostStyle(color: string, fade: number) {
    this.ghostStyle = { color, fade };
    this.applyPalette();
    this.render();
  }

  setGhost(enabled: boolean) {
    this.showGhost = enabled;
    this.ghostGroup.visible = enabled && this.progress.k < this.segs.length;
    this.render();
  }

  private colorPaths() {
    const selected = new THREE.Color(this.pal.hl);
    const muted = new THREE.Color(this.pal.dim);
    const normal = new THREE.Color();
    for (const { line, idx, tool } of this.bundles) {
      normal.set(tool < 0 ? this.pal.rapid : this.pal.tools[tool % TOOL_COUNT]);
      const colors = line.geometry.getAttribute("color");
      for (let i = 0; i < idx.length; i++) {
        const c = !this.codeRows ? normal : this.codeRows.has(this.segs[idx[i]].expanded_index) ? selected : muted;
        colors.setXYZ(i * 2, c.r, c.g, c.b);
        colors.setXYZ(i * 2 + 1, c.r, c.g, c.b);
      }
      colors.needsUpdate = true;
    }
    // 選んだ区間を太線で重ねる
    const idx: number[] = [], pos: number[] = [];
    if (this.codeRows) {
      this.segs.forEach((s, i) => {
        if (!this.codeRows!.has(s.expanded_index)) return;
        idx.push(i);
        pos.push(...this.rel(s.from), ...this.rel(s.to));
      });
    }
    this.sel.geometry.dispose();
    this.sel.geometry = pos.length ? new LineSegmentsGeometry().setPositions(pos) : new LineSegmentsGeometry();
    this.selIdx = Int32Array.from(idx);
    this.sel.visible = idx.length > 0;
    this.drawSel(this.progress.k);
  }

  /** 選んだ区間の太線を、線分 k の手前まで描く */
  private drawSel(k: number) {
    let lo = 0, hi = this.selIdx.length;
    while (lo < hi) { const m = (lo + hi) >> 1; if (this.selIdx[m] < k) lo = m + 1; else hi = m; }
    this.sel.geometry.instanceCount = lo;
  }

  /** 再生位置を描く：線分 k の手前までと、線分 k の割合 frac まで。工具の印を先端に置く。 */
  setProgress(k: number, frac: number) {
    this.progress = { k, frac };
    this.ghostGroup.visible = this.showGhost && k < this.segs.length;
    for (const b of this.bundles) {
      // idx の中で k より小さい数 ＝ 描く本数（二分探索）
      let lo = 0, hi = b.idx.length;
      while (lo < hi) { const m = (lo + hi) >> 1; if (b.idx[m] < k) lo = m + 1; else hi = m; }
      b.line.geometry.setDrawRange(0, lo * 2);
    }
    this.drawSel(k);
    const s = this.segs[k];
    const pos = this.partial.geometry.getAttribute("position") as THREE.BufferAttribute;
    if (s) {
      const a = this.rel(s.from), b = this.rel(s.to);
      const p = [0, 1, 2].map((i) => a[i] + (b[i] - a[i]) * frac);
      pos.set([...a, ...p]);
      const color = this.codeRows
        ? this.codeRows.has(s.expanded_index) ? this.pal.hl : this.pal.dim
        : s.kind === "Rapid" ? this.pal.rapid : this.pal.tools[s.tool_index % TOOL_COUNT];
      this.partial.material.color.set(color);
      this.tool.position.set(p[0], p[1], p[2]);
    } else {
      pos.set(new Array(6).fill(0));
      const last = this.segs[this.segs.length - 1];
      if (last) this.tool.position.set(...this.rel(last.to));
    }
    pos.needsUpdate = true;
    this.partial.visible = !!s;
    this.tool.visible = this.segs.length > 0;
    this.render();
  }

  /** 再生をやめて経路の全体を描く。 */
  clearProgress() {
    for (const b of this.bundles) b.line.geometry.setDrawRange(0, Infinity);
    this.partial.visible = false;
    this.progress = { k: this.segs.length, frac: 0 };
    this.drawSel(this.segs.length);
    this.ghostGroup.visible = false;
    this.tool.visible = false;
    this.render();
  }

  /** テーマが変わったら色を読み直して描き直す（呼び出し側で再生位置を戻す）。 */
  setTheme() {
    this.pal = readPalette();
    this.applyPalette();
    this.colorPaths();
    this.highlightLine(this.highlightedLine);
  }

  private applyPalette() {
    this.renderer.setClearColor(this.pal.bg);
    const ghost = this.ghostStyle.color || this.pal.ghost, opacity = 1 - this.ghostStyle.fade;
    for (const m of [this.ghostMaterial, this.ghostRapid]) {
      m.color.set(ghost);
      m.opacity = opacity;
      m.transparent = opacity < 1;
      m.needsUpdate = true;
    }
    this.hlMat.color.set(this.pal.hl);
    this.axes.setColors(...(this.pal.axes.map((c) => new THREE.Color(c)) as [THREE.Color, THREE.Color, THREE.Color]));
    const [body, edge] = this.tool.children as [THREE.Mesh, THREE.LineSegments];
    (body.material as THREE.MeshBasicMaterial).color.set(this.pal.tool);
    (edge.material as THREE.LineBasicMaterial).color.set(this.pal.toolEdge);
  }

  /** 各ビューの視点・倍率・注視点（ワーク座標）と分け方。設定ファイルに残す。 */
  getState() {
    const ports = Object.fromEntries(
      this.allPorts().map((p) => [p.name, { az: p.az, el: p.el, scale: p.scale, target: p.target.clone().add(this.center).toArray() as [number, number, number] }]),
    );
    return { layout: this.layout, active: this.active, maximized: this.maximized, split: { ...this.split }, ports };
  }

  /** getState の値を戻す。経路を読んだ後に呼ぶ（注視点は今の表示中心からの相対に直す）。 */
  setState(st: { layout: Layout; active: ViewName; maximized: boolean; split?: { x: number; y: number }; ports: Record<string, { az: number; el: number; scale: number; target: [number, number, number] }> }) {
    this.layout = st.layout === "quad" ? "quad" : "single";
    const sp = st.split;
    if (sp && [sp.x, sp.y].every((r) => Number.isFinite(r) && r > 0 && r < 1)) this.split = { x: sp.x, y: sp.y };
    this.active = st.active in this.ports ? st.active : "xy";
    this.maximized = this.layout === "single" && !!st.maximized;
    for (const p of this.allPorts()) {
      const v = st.ports[p.name];
      if (!v || ![v.az, v.el, v.scale, ...v.target].every(Number.isFinite) || v.scale <= 0) continue;
      ({ az: p.az, el: p.el, scale: p.scale } = v);
      p.target.set(...v.target).sub(this.center);
    }
    this.relayout();
    this.render();
    this.onLayout(this.layout, this.active, this.maximized);
  }

  /** 1 画面で見る視点を選ぶ（視点を初期の向きに戻してフィット）。 */
  setView(v: ViewName) {
    this.active = v;
    this.maximized = false;
    this.layout = "single";
    const p = this.ports[v];
    ({ az: p.az, el: p.el } = VIEWS[v]);
    this.relayout();
    this.fitPort(p);
    this.onChange();
  }

  /** 1 画面と 4 分割の切り替え。区画の大きさが変わるので、見えるビューをフィットする。
   *  4 分割の中での最大化・復帰（ダブルクリック）では各ビューの視点と倍率を保つ。 */
  setLayout(layout: Layout) {
    this.layout = layout;
    this.maximized = false;
    this.relayout();
    this.fit();
    this.onChange();
  }

  /** 経路の全体が入るように、見えているビューの注視点と倍率を決める（フィット）。 */
  fit() {
    for (const p of this.visiblePorts()) this.fitPort(p);
    this.onChange();
  }

  private fitPort(p: Port) {
    p.update();
    const { w, h } = p.rect;
    const box = new THREE.Box3();
    for (const s of this.segs) {
      box.expandByPoint(new THREE.Vector3(...this.rel(s.from)));
      box.expandByPoint(new THREE.Vector3(...this.rel(s.to)));
    }
    if (box.isEmpty()) {
      p.target.set(0, 0, 0);
      p.scale = Math.min(w, h) / 100;
      p.update();
      return this.render();
    }
    // 箱の 8 隅を画面の軸へ投影して幅と高さを測る
    const right = new THREE.Vector3().setFromMatrixColumn(p.camera.matrixWorld, 0);
    const up = new THREE.Vector3().setFromMatrixColumn(p.camera.matrixWorld, 1);
    let rmin = Infinity, rmax = -Infinity, umin = Infinity, umax = -Infinity;
    for (let i = 0; i < 8; i++) {
      const c = new THREE.Vector3(i & 1 ? box.max.x : box.min.x, i & 2 ? box.max.y : box.min.y, i & 4 ? box.max.z : box.min.z);
      const r = c.dot(right), u = c.dot(up);
      rmin = Math.min(rmin, r); rmax = Math.max(rmax, r);
      umin = Math.min(umin, u); umax = Math.max(umax, u);
    }
    box.getCenter(p.target);
    // 視線の方向にしか広がらない経路（XY で Z だけ動く など）でも倍率が飛ばないように下限を置く
    const floor = Math.max(box.getSize(new THREE.Vector3()).length() * 0.25, 10);
    p.scale = Math.min(w / Math.max(rmax - rmin, floor), h / Math.max(umax - umin, floor)) * 0.88;
    p.update();
    this.render();
  }

  private rel(p: [number, number, number]): [number, number, number] {
    return [p[0] - this.center.x, p[1] - this.center.y, p[2] - this.center.z];
  }

  private allPorts() { return QUAD.map((n) => this.ports[n]); }

  private visiblePorts() {
    return this.layout === "quad" ? this.allPorts() : [this.ports[this.active]];
  }

  /** ビューの区画を決める。見えないビューにも 4 分割の区画を持たせ、フィットの大きさの基準にする。 */
  private relayout() {
    const W = Math.max(this.host.clientWidth, 1), H = Math.max(this.host.clientHeight, 1);
    // 境界は各ビューが MIN_PANE 以上残る範囲に（窓が狭いときは中央）
    const at = (r: number, n: number) => Math.round(n > 2 * MIN_PANE ? Math.min(n - MIN_PANE, Math.max(MIN_PANE, n * r)) : n / 2);
    const hw = at(this.split.x, W), hh = at(this.split.y, H);
    QUAD.forEach((n, i) => {
      const p = this.ports[n];
      const quad = { x: i % 2 ? hw : 0, y: i < 2 ? 0 : hh, w: i % 2 ? W - hw : hw, h: i < 2 ? hh : H - hh };
      const shown = this.layout === "quad" || n === this.active;
      p.rect = this.layout === "quad" ? quad : n === this.active ? { x: 0, y: 0, w: W, h: H } : quad;
      Object.assign(p.frame.style, { left: `${p.rect.x}px`, top: `${p.rect.y}px`, width: `${p.rect.w}px`, height: `${p.rect.h}px` });
      p.frame.style.display = shown && this.layout === "quad" ? "" : "none";
      p.update();
    });
    const quad = this.layout === "quad";
    const { v, h, c } = this.bars;
    for (const b of [v, h, c]) b.style.display = quad ? "" : "none";
    Object.assign(v.style, { left: `${hw - BAR / 2}px`, top: "0px", width: `${BAR}px`, height: `${H}px` });
    Object.assign(h.style, { left: "0px", top: `${hh - BAR / 2}px`, width: `${W}px`, height: `${BAR}px` });
    Object.assign(c.style, { left: `${hw - BAR}px`, top: `${hh - BAR}px`, width: `${BAR * 2}px`, height: `${BAR * 2}px` });
  }

  /** 4 分割の境界を作る。ドラッグで割合を変え、ダブルクリックで均等に戻す。 */
  private makeSplitBars() {
    const make = (k: "v" | "h" | "c") => {
      const b = document.createElement("div");
      b.className = `split-bar split-${k}`;
      b.title = "ドラッグで大きさを変更。ダブルクリックで均等に";
      this.host.append(b);
      b.addEventListener("pointerdown", (e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        b.setPointerCapture(e.pointerId);
        b.classList.add("dragging");
      });
      b.addEventListener("pointermove", (e) => {
        if (!b.hasPointerCapture(e.pointerId)) return;
        const r = this.host.getBoundingClientRect();
        if (k !== "h") this.split.x = Math.min(1, Math.max(0, (e.clientX - r.left) / Math.max(r.width, 1)));
        if (k !== "v") this.split.y = Math.min(1, Math.max(0, (e.clientY - r.top) / Math.max(r.height, 1)));
        this.relayout();
        this.render();
      });
      const end = (e: PointerEvent) => {
        if (!b.hasPointerCapture(e.pointerId)) return;
        b.releasePointerCapture(e.pointerId);
        b.classList.remove("dragging");
        this.clampSplit();
        this.onChange();
      };
      b.addEventListener("pointerup", end);
      b.addEventListener("pointercancel", end);
      b.addEventListener("dblclick", () => {
        if (k !== "h") this.split.x = 0.5;
        if (k !== "v") this.split.y = 0.5;
        this.relayout();
        this.render();
        this.onChange();
      });
      return b;
    };
    return { v: make("v"), h: make("h"), c: make("c") };
  }

  /** 保存する割合を、実際に描いた境界の位置にそろえる。 */
  private clampSplit() {
    const W = Math.max(this.host.clientWidth, 1), H = Math.max(this.host.clientHeight, 1);
    if (W > 2 * MIN_PANE) this.split.x = Math.min(W - MIN_PANE, Math.max(MIN_PANE, W * this.split.x)) / W;
    if (H > 2 * MIN_PANE) this.split.y = Math.min(H - MIN_PANE, Math.max(MIN_PANE, H * this.split.y)) / H;
  }

  private resize() {
    this.renderer.setSize(Math.max(this.host.clientWidth, 1), Math.max(this.host.clientHeight, 1));
    this.relayout();
    this.render();
  }

  private render() {
    cancelAnimationFrame(this.frame);
    this.frame = requestAnimationFrame(() => {
      const H = Math.max(this.host.clientHeight, 1);
      for (const p of this.visiblePorts()) {
        const { x, y, w, h } = p.rect;
        this.renderer.setViewport(x, H - y - h, w, h);
        this.renderer.setScissor(x, H - y - h, w, h);
        this.hlMat.resolution.set(w, h);
        // 工具の印と破線の間隔は、そのビューの画面上で一定に
        this.tool.scale.setScalar(1 / p.scale);
        for (const m of this.dashed) {
          m.dashSize = 6 / p.scale;
          m.gapSize = 4 / p.scale;
        }
        this.renderer.render(this.scene, p.camera);
      }
    });
  }

  /** host 内の画素 → そのビューと、ビュー内の画素。 */
  private portAt(x: number, y: number) {
    for (const p of this.visiblePorts()) {
      const r = p.rect;
      if (x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h) return { p, x: x - r.x, y: y - r.y };
    }
    return null;
  }

  /** ビュー内で 7px 以内の最も近い線分。 */
  private pick(p: Port, x: number, y: number): Segment | null {
    const { w, h } = p.rect;
    const toScreen = (q: [number, number, number]) => {
      const v = new THREE.Vector3(...this.rel(q)).project(p.camera);
      return [((v.x + 1) / 2) * w, ((1 - v.y) / 2) * h];
    };
    let best: Segment | null = null, bestD = 7;
    for (const s of this.segs) {
      const [ax, ay] = toScreen(s.from), [bx, by] = toScreen(s.to);
      const dx = bx - ax, dy = by - ay, len2 = dx * dx + dy * dy;
      const t = len2 ? Math.max(0, Math.min(1, ((x - ax) * dx + (y - ay) * dy) / len2)) : 0;
      const d = Math.hypot(x - (ax + t * dx), y - (ay + t * dy));
      if (d < bestD) { bestD = d; best = s; }
    }
    return best;
  }

  /** 4 分割のビューを最大化、または 4 分割へ戻す（左ダブルクリック）。 */
  private toggleMaximize(p: Port) {
    if (this.layout === "quad") {
      this.layout = "single";
      this.active = p.name;
      this.maximized = true;
    } else if (this.maximized) {
      this.layout = "quad";
      this.maximized = false;
    } else return;
    this.relayout();
    this.render();
    this.onLayout(this.layout, this.active, this.maximized);
    this.onChange();
  }

  private bindMouse() {
    const el = this.renderer.domElement;
    let drag: { p: Port; btn: number; x: number; y: number; moved: boolean } | null = null;
    let lastMiddle = { t: 0, p: null as Port | null };
    const local = (e: MouseEvent) => {
      const r = el.getBoundingClientRect();
      return this.portAt(e.clientX - r.left, e.clientY - r.top);
    };
    el.addEventListener("contextmenu", (e) => e.preventDefault());
    // 中ボタンの既定（自動スクロール）を止める
    el.addEventListener("mousedown", (e) => { if (e.button === 1) e.preventDefault(); });
    const touch = this.bindTouch(el, local);
    el.addEventListener("pointerdown", (e) => {
      if (touch(e)) return;
      const hit = local(e);
      if (!hit) return;
      if (e.button === 1) {
        // 中ボタンのダブルクリック：そのビューをフィット
        if (e.timeStamp - lastMiddle.t < 400 && lastMiddle.p === hit.p) {
          this.fitPort(hit.p);
          this.onChange();
          lastMiddle = { t: 0, p: null };
          return;
        }
        lastMiddle = { t: e.timeStamp, p: hit.p };
      }
      drag = { p: hit.p, btn: e.button, x: e.clientX, y: e.clientY, moved: false };
      el.setPointerCapture(e.pointerId);
    });
    el.addEventListener("pointermove", (e) => {
      if (touch(e)) return;
      const hit = local(e);
      if (hit && VIEWS[hit.p.name].axes && (hit.p.name !== "3d")) {
        const w = hit.p.screenToWorld(hit.x, hit.y).add(this.center);
        const v = { X: w.x, Y: w.y, Z: w.z } as Record<string, number>;
        const [a, b] = hit.p.top ? ["X", "Y"] : hit.p.name === "xz" ? ["X", "Z"] : hit.p.name === "yz" ? ["Y", "Z"] : ["", ""];
        this.onHover(a ? `${a} ${v[a].toFixed(3)}  ${b} ${v[b].toFixed(3)}` : "");
      } else this.onHover("");
      if (!drag) return;
      const p = drag.p;
      const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
      if (Math.abs(dx) + Math.abs(dy) > 2) drag.moved = true;
      drag.x = e.clientX;
      drag.y = e.clientY;
      if (drag.btn === 2) {
        if (p.top) p.el = Math.sign(p.el) * 89.9;
        p.az -= dx * 0.4;
        p.el = Math.max(-89.9, Math.min(89.9, p.el + dy * 0.4));
      } else {
        const right = new THREE.Vector3().setFromMatrixColumn(p.camera.matrixWorld, 0);
        const up = new THREE.Vector3().setFromMatrixColumn(p.camera.matrixWorld, 1);
        p.target.addScaledVector(right, -dx / p.scale).addScaledVector(up, dy / p.scale);
      }
      p.update();
      this.render();
    });
    el.addEventListener("pointerup", (e) => {
      if (touch(e)) return;
      if (drag && !drag.moved && drag.btn === 0) {
        const hit = local(e);
        const s = hit && hit.p === drag.p ? this.pick(hit.p, hit.x, hit.y) : null;
        if (s) this.onPick(s.src_line);
      }
      if (drag && !drag.moved && drag.btn === 2) this.onContextMenu(e);
      if (drag?.moved) this.onChange();
      drag = null;
    });
    el.addEventListener("pointercancel", (e) => void touch(e));
    el.addEventListener("dblclick", (e) => {
      if (touch.recent()) return; // タッチのダブルタップはフィット（bindTouch）
      const hit = local(e);
      if (hit && e.button === 0) this.toggleMaximize(hit.p);
    });
    el.addEventListener(
      "wheel",
      (e) => {
        e.preventDefault();
        const hit = local(e);
        if (!hit) return;
        this.zoomAt(hit.p, hit.x, hit.y, Math.pow(1.0015, -e.deltaY));
        this.render();
        this.onChange();
      },
      { passive: false },
    );
  }

  /** ビュー内の点を固定して拡大・縮小（ホイール・ピンチ） */
  private zoomAt(p: Port, x: number, y: number, factor: number) {
    const before = p.screenToWorld(x, y);
    p.scale = Math.min(1e6, Math.max(1e-4, p.scale * factor));
    p.update();
    p.target.add(before.sub(p.screenToWorld(x, y)));
    p.update();
  }

  /** 画面の向きに沿って注視点を動かす（画素） */
  private panBy(p: Port, dx: number, dy: number) {
    const right = new THREE.Vector3().setFromMatrixColumn(p.camera.matrixWorld, 0);
    const up = new THREE.Vector3().setFromMatrixColumn(p.camera.matrixWorld, 1);
    p.target.addScaledVector(right, -dx / p.scale).addScaledVector(up, dy / p.scale);
  }

  /** タッチの操作。タッチの指のイベントなら処理して true（マウスの処理へ渡さない） */
  private bindTouch(el: HTMLElement, local: (e: MouseEvent) => { p: Port; x: number; y: number } | null) {
    const pts = new Map<number, { x: number; y: number }>();
    let g: { p: Port; x: number; y: number; t: number; moved: boolean } | null = null;
    let pinch: { x: number; y: number; d: number } | null = null;
    let lastTap = { t: 0, x: 0, y: 0 }, lastTouch = 0;
    const mid = () => {
      const [a, b] = [...pts.values()];
      return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2, d: Math.max(1, Math.hypot(a.x - b.x, a.y - b.y)) };
    };
    const handle = (e: PointerEvent) => {
      if (e.pointerType !== "touch" || !this.touchGestures) return false;
      lastTouch = e.timeStamp;
      const at = { x: e.clientX, y: e.clientY };
      if (e.type === "pointerdown") {
        pts.set(e.pointerId, at);
        el.setPointerCapture(e.pointerId);
        if (pts.size === 1) {
          const hit = local(e);
          g = hit && { p: hit.p, x: at.x, y: at.y, t: e.timeStamp, moved: false };
        } else if (g) {
          g.moved = true;
          pinch = mid();
        }
        return true;
      }
      const prev = pts.get(e.pointerId);
      if (!prev) return true;
      if (e.type === "pointermove") {
        pts.set(e.pointerId, at);
        if (!g) return true;
        const p = g.p;
        if (pts.size >= 2 && pinch) {
          const m = mid(), r = el.getBoundingClientRect();
          this.panBy(p, m.x - pinch.x, m.y - pinch.y);
          p.update();
          this.zoomAt(p, m.x - r.left - p.rect.x, m.y - r.top - p.rect.y, m.d / pinch.d);
          pinch = m;
        } else {
          if (Math.hypot(at.x - g.x, at.y - g.y) > 6) g.moved = true;
          if (!g.moved) return true;
          const dx = at.x - prev.x, dy = at.y - prev.y;
          if (p.name === "3d") {
            if (p.top) p.el = Math.sign(p.el) * 89.9;
            p.az -= dx * 0.4;
            p.el = Math.max(-89.9, Math.min(89.9, p.el + dy * 0.4));
          } else this.panBy(p, dx, dy);
        }
        p.update();
        this.render();
        return true;
      }
      // pointerup・pointercancel
      pts.delete(e.pointerId);
      if (pts.size === 1) pinch = null;
      if (pts.size || !g) return true;
      const { p, moved, t } = g;
      g = pinch = null;
      if (moved) this.onChange();
      else if (e.type === "pointerup" && e.timeStamp - t < 500) {
        if (e.timeStamp - lastTap.t < 350 && Math.hypot(at.x - lastTap.x, at.y - lastTap.y) < 30) {
          this.fitPort(p);
          this.onChange();
          lastTap.t = 0;
        } else {
          lastTap = { t: e.timeStamp, ...at };
          const hit = local(e);
          const s = hit && hit.p === p ? this.pick(p, hit.x, hit.y) : null;
          if (s) this.onPick(s.src_line);
        }
      }
      return true;
    };
    return Object.assign(handle, { recent: () => this.touchGestures && performance.now() - lastTouch < 800 });
  }
}

/** 工具の印：先端を原点に置いた円柱（画面上で直径 12px・長さ 36px）。上から見ると円。 */
function makeTool() {
  const g = new THREE.Group();
  const body = new THREE.Mesh(
    new THREE.CylinderGeometry(6, 6, 36, 20).rotateX(Math.PI / 2).translate(0, 0, 18),
    new THREE.MeshBasicMaterial({ color: 0xd0d6e0, transparent: true, opacity: 0.35, depthTest: false }),
  );
  const edge = new THREE.LineSegments(
    new THREE.EdgesGeometry(body.geometry, 30),
    new THREE.LineBasicMaterial({ color: 0xffffff, depthTest: false }),
  );
  const tip = new THREE.Points(
    new THREE.BufferGeometry().setAttribute("position", new THREE.Float32BufferAttribute([0, 0, 0], 3)),
    new THREE.PointsMaterial({ color: 0xff5050, size: 7, sizeAttenuation: false, depthTest: false }),
  );
  g.add(body, edge, tip);
  for (const c of g.children) c.renderOrder = 10;
  return g;
}
