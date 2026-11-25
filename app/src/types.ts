// nclens-core の公開型（crates/nclens-core/src/lib.rs）と同じ形。serde の既定の JSON。

/** 6 軸の値：X Y Z A B C */
export type Axes6 = [number, number, number, number, number, number];
/** ブロックの実行後のモーダル状態。g_codes はモーダル群の番号 → G コード */
export interface ModalSnapshot {
  g_codes: Record<string, number>; feed: number; spindle: number; tool: number; h: number; d: number;
  work_coordinate: number; plane: number; absolute: boolean; position: Axes6;
}
export interface AxisRange { min: Axes6; max: Axes6 }
export interface CoordinateBounds { work: AxisRange; machine: AxisRange }
export interface ExpandedLine {
  text: string; src_line: number; call_stack: string[]; modal: ModalSnapshot;
  source_g_codes?: Record<string, number>;
}
export type SegKind = "Rapid" | "Cut";
export interface Segment { kind: SegKind; from: [number, number, number]; to: [number, number, number]; src_line: number; tool_index: number; time_sec: number; expanded_index: number }
export interface VarChange { src_line: number; var: number; value: number | null }
export interface Diagnostic { src_line: number; message: string }
export interface Stats { cut_length_mm: number; rapid_length_mm: number; time_sec: number; min: [number, number, number]; max: [number, number, number] }
export interface StopPoint { expanded_index: number; src_line: number; kind: "M00" | "M01" }
export interface CallEvent { src_line: number; kind: "M98" | "M99"; expanded_index_after: number }
export interface BlockSkipUsage { level: number; line_count: number }
export interface RunResult {
  expanded: ExpandedLine[]; path: Segment[]; var_trace: VarChange[]; diagnostics: Diagnostic[]; stats: Stats;
  stops?: StopPoint[];
  calls?: CallEvent[];
  /** NC に出るブロックスキップの番号と行数（番号順。無ければ省略） */
  block_skip_usage?: BlockSkipUsage[];
  /** 解析・描画・工具認識の警告（展開は継続） */
  warnings?: Diagnostic[];
  bounds: CoordinateBounds; tool_bounds: Record<string, CoordinateBounds>;
}

// レポート（nclens-core の Report。make_report の結果）
export interface MotionTotals {
  cut_length_mm: number; rapid_length_mm: number; other_length_mm: number;
  cut_time_sec: number; rapid_time_sec: number; other_time_sec: number;
}
export interface ReportBreakdown { totals: MotionTotals; bounds: CoordinateBounds; used_lines: number[] }
export interface Report {
  filename: string; generated_at_unix_seconds: number; mnemonic: string; post_name: string;
  totals: MotionTotals; feed_min: number | null; feed_max: number | null; bounds: CoordinateBounds;
  tools: Record<string, ReportBreakdown>; programs: Record<string, ReportBreakdown>;
  errors: number; warnings: number;
}

// ニーモニック・固有（nclens-core の DialectSettings。.dialect.toml・JSON の中身と同じ形）
export type MacroSyntax = "fanuc" | "osp";
/** マクロ記号の表の行（nclens-core の macro_settings.rs と同じ形） */
export interface VariableDefinition { prefix: string; kind: "empty" | "local" | "common" | "retained" | "system"; range?: [number, number]; offset?: number; description: string }
export interface OperatorDefinition { symbol: string; operation: string; precedence: number; description: string }
export interface ControlDefinition { keyword: string; statement: string; description: string }
export interface FunctionDefinition { name: string; function: string; arity: [number, number]; angle_unit?: "degrees" | "radians"; description: string }
/** マクロ記号（[macro]。省略の項目：組み込みの FANUC・OSP の値） */
export interface MacroSettings {
  syntax?: MacroSyntax;
  expression?: [string, string];
  arguments?: [string, string];
  assignment?: string;
  block_skip?: string;
  comment_pair?: string[];
  line_comments?: string[];
  variables?: VariableDefinition[];
  operators?: OperatorDefinition[];
  comparisons?: OperatorDefinition[];
  controls?: ControlDefinition[];
  functions?: FunctionDefinition[];
}
/** ヘルプの一覧表（nclens-core の MacroReference） */
export interface MacroReferenceRow { name: string; symbol: string; description: string; example: string }
export interface MacroReference { variables: MacroReferenceRow[]; operators: MacroReferenceRow[]; comparisons: MacroReferenceRow[]; controls: MacroReferenceRow[]; functions: MacroReferenceRow[] }
export const ADDRESS_ROLES = ["X", "Y", "Z", "A", "B", "C", "U", "V", "W", "I", "J", "K", "R", "F", "S", "T", "H", "D", "O", "N", "P", "L", "Q", "G", "M", "DwellSeconds", "Repeat"] as const;
export type AddressRole = (typeof ADDRESS_ROLES)[number];
/** description：ツールチップの解説（空・省略：基底のニーモニック・固有の解説） */
export interface GCodeDefinition { function: string; group: number | null; description?: string }
export interface MCodeDefinition { function: string; program: number | null; description?: string }
export type CustomCode = { call: number } | { replace: string };
export interface DialectSettings {
  name: string;
  base: string | null;
  macro?: MacroSettings;
  addresses: Record<string, AddressRole>;
  g_codes: Record<string, GCodeDefinition>;
  m_codes: Record<string, MCodeDefinition>;
  custom: Record<string, CustomCode>;
  /** アドレスの文字 → ツールチップの解説 */
  address_descriptions?: Record<string, string>;
}

export type DuplicateWord = "last" | "first" | "error";
// ポスト（nclens-core の PostProfile。.post.toml・JSON の中身と同じ形）
export type Xyz = [number, number, number];
export interface ToolOffset { h_geometry: number; h_wear: number; d_geometry: number; d_wear: number }
export interface PostProfile {
  mnemonic: DialectSettings;
  version: number;
  name: string;
  comment: string;
  codes: {
    /** 同一ブロックの同じアドレス・同じモーダル群の G：後方優先・先方優先・エラー */
    duplicate_word: DuplicateWord; duplicate_g_group: DuplicateWord;
    /** 1 ブロックの M の上限と、複数の M の扱い */
    max_m_per_block: number; duplicate_m: "all" | "last" | "first";
    warn_on_duplicate: boolean;
    rapid_mm_min: number; max_rpm: number; atc_sec: number; compensation_sec: number;
    g73_retract_mm: number; g83_clearance_mm: number; decimal_places: number;
    integer_units: "least_increment" | "millimeters";
    compensation: "FANUC-A" | "FANUC-B" | "FANUC-C";
    tool_change: "t" | "m6";
    macro_variables: "15M" | "16M" | "6M";
    m_calls: Record<string, number>; g_calls: Record<string, number>;
    t_call: boolean; s_call: boolean; b_call: boolean;
    ignore_g: number[]; ignore_m: number[];
    program_address: string;
  };
  variables: {
    common: Record<string, number>;
    work: [Xyz, Xyz, Xyz, Xyz, Xyz, Xyz];
    external: Xyz;
    offsets: Record<string, ToolOffset>;
    g28: Xyz;
    g30: [Xyz, Xyz, Xyz];
    tools: Record<string, { diameter_mm?: number | null; name?: string | null }>;
  };
  expand: {
    cycles: boolean; g10: boolean; decimal_point: boolean; omit_zero_moves: boolean;
    distance_mode: "preserve" | "absolute" | "incremental"; block_skip: boolean; block_skip_levels: number[];
  };
}
