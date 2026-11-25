// wasm-bindgen の出力は Web ビルド時に生成する。
declare module "nclens-wasm" {
  export default function init(): Promise<unknown>;
  export function run_nc(text: string, post: string, timestamp: bigint, blockSkip?: boolean): string;
  export function run_nc_with_skip_levels(text: string, post: string, timestamp: bigint, levels: Uint8Array | number[]): string;
  export function run_through_stops(text: string, post: string): string;
  export function decode_file(bytes: Uint8Array): string;
  export function encode_file(text: string, encoding: string): Uint8Array<ArrayBuffer>;
  export function post_default(): string;
  export function parse_post(text: string): string;
  export function format_post(post: string): string;
  export function builtin_dialects(): string;
  export function parse_dialect(text: string): string;
  export function format_dialect(dialect: string): string;
  export function validate_dialect(dialect: string): string;
  export function parse_post_json(text: string): string;
  export function format_post_json(post: string): string;
  export function parse_dialect_json(text: string): string;
  export function format_dialect_json(dialect: string): string;
  export function macro_reference(mnemonic: string): string;
}
