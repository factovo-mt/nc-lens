// シミュレーションの再生。時間軸は線分ごとの所要時間（Segment.time_sec）の累積。
// requestAnimationFrame で位置を進め、位置が変わるたびに onUpdate へ渡す（描画・強調は呼び出し側）。
import type { Segment, StopPoint } from "./types";

export interface PlayPos {
  /** 今の線分の番号（末尾では segs.length） */
  k: number;
  /** 線分 k の中の割合 0〜1 */
  frac: number;
  /** 経過時間（秒） */
  t: number;
}

export class Player {
  private segs: Segment[] = [];
  /** cum[i] ＝ 線分 i の開始時刻。cum[n] ＝ 合計 */
  private cum = new Float64Array(1);
  private t = 0;
  private dir: -1 | 0 | 1 = 0;
  private last = 0;
  private frame = 0;
  private stops: { time: number; point: StopPoint }[] = [];
  private nextStop = 0;
  stoppedAt: StopPoint | null = null;
  optionalStop = false;
  speed = 1;

  constructor(private onUpdate: (p: PlayPos) => void, private onState: (dir: -1 | 0 | 1) => void) {}

  get total() { return this.cum[this.cum.length - 1]; }
  get time() { return this.t; }
  get playing() { return this.dir; }

  /** 経路を入れ替え、末尾（全体の表示）に置く。 */
  load(segs: Segment[], stops: StopPoint[] = []) {
    this.stop();
    this.segs = segs;
    this.cum = new Float64Array(segs.length + 1);
    // 時間 0 の線分（F0 など）も 1 歩として見えるよう、ごく短い時間を与える
    for (let i = 0; i < segs.length; i++) this.cum[i + 1] = this.cum[i] + Math.max(segs[i].time_sec, 1e-4);
    let i = 0;
    this.stops = stops.map((point) => {
      while (i < segs.length && segs[i].expanded_index <= point.expanded_index) i++;
      return { time: this.cum[i], point };
    });
    this.stoppedAt = null;
    this.nextStop = this.stops.length;
    this.t = this.total;
  }

  play(dir: -1 | 1) {
    if (!this.segs.length && (dir < 0 || !this.stops.length)) return;
    // 端から逆向きには進めないので、反対の端へ戻して始める
    if (dir > 0 && this.t >= this.total && !this.stoppedAt) {
      this.t = 0;
      this.nextStop = 0;
    }
    if (dir < 0 && this.t <= 0) this.t = this.total;
    this.stoppedAt = null;
    this.dir = dir;
    this.last = performance.now();
    cancelAnimationFrame(this.frame);
    this.frame = requestAnimationFrame(this.tick);
    this.onState(dir);
    this.emit();
  }

  stop() {
    cancelAnimationFrame(this.frame);
    if (this.dir !== 0) { this.dir = 0; this.onState(0); }
  }

  seek(t: number) {
    this.t = Math.min(this.total, Math.max(0, t));
    this.stoppedAt = null;
    this.rewindStops();
    this.emit();
  }

  /** 1 ブロック（同じ元行が続く線分のまとまり）進める・戻す。 */
  step(dir: -1 | 1) {
    this.stop();
    const n = this.segs.length;
    if (!n) return;
    let { k } = this.at(this.t);
    if (dir > 0) {
      if (k >= n) return;
      const line = this.segs[k].src_line;
      while (k < n && this.segs[k].src_line === line) k++;
      this.seek(this.cum[k]);
    } else {
      // 線分の途中なら、そのブロックの頭へ。頭にいれば、前のブロックの頭へ
      if (k >= n || this.t <= this.cum[k] + 1e-12) k--;
      if (k < 0) return this.seek(0);
      const line = this.segs[k].src_line;
      while (k > 0 && this.segs[k - 1].src_line === line) k--;
      this.seek(this.cum[k]);
    }
  }

  /** 時刻 t の線分と割合（二分探索）。 */
  at(t: number): PlayPos {
    const n = this.segs.length;
    if (t >= this.total) return { k: n, frac: 0, t };
    let lo = 0, hi = n - 1;
    while (lo < hi) { const m = (lo + hi + 1) >> 1; if (this.cum[m] <= t) lo = m; else hi = m - 1; }
    const d = this.cum[lo + 1] - this.cum[lo];
    return { k: lo, frac: d > 0 ? (t - this.cum[lo]) / d : 1, t };
  }

  private emit() { this.onUpdate(this.at(this.t)); }

  private tick = (now: number) => {
    // rAF の時刻は play() の時点より前のことがあるので負を捨てる。裏に回った後の大きな飛びも抑える
    const dt = Math.min(0.25, Math.max(0, (now - this.last) / 1000));
    this.last = now;
    const next = this.t + dt * this.speed * this.dir;
    if (this.dir > 0) {
      while (this.nextStop < this.stops.length && this.stops[this.nextStop].time <= next) {
        const { time, point } = this.stops[this.nextStop++];
        if (point.kind === "M01" && !this.optionalStop) continue;
        this.t = time;
        this.stoppedAt = point;
        this.stop();
        this.emit();
        return;
      }
    }
    this.t = next;
    const end = this.dir > 0 ? this.t >= this.total : this.t <= 0;
    this.t = Math.min(this.total, Math.max(0, this.t));
    if (this.dir < 0) this.rewindStops();
    this.emit();
    if (end) this.stop();
    else this.frame = requestAnimationFrame(this.tick);
  };

  private rewindStops() {
    let lo = 0, hi = this.stops.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (this.stops[mid].time < this.t) lo = mid + 1;
      else hi = mid;
    }
    this.nextStop = lo;
  }
}
