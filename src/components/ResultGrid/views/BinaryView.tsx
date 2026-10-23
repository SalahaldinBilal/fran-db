import styles from "./Views.module.scss";
import { For, Show, createMemo } from "solid-js";
import { formatBytes } from "@core/helpers";
import Facts, { Fact } from "./Facts";

const BYTES_PER_ROW = 16;
const MAX_ROWS = 2048;

const SIGNATURES: Array<[string, number[]]> = [
  ["PNG image", [0x89, 0x50, 0x4e, 0x47]],
  ["JPEG image", [0xff, 0xd8, 0xff]],
  ["GIF image", [0x47, 0x49, 0x46, 0x38]],
  ["PDF document", [0x25, 0x50, 0x44, 0x46]],
  ["ZIP archive", [0x50, 0x4b, 0x03, 0x04]],
  ["gzip data", [0x1f, 0x8b]],
];

function parseHex(value: string): Uint8Array | null {
  const body = value.trim().replace(/^0x/i, "");
  if (body.length % 2 !== 0 || !/^[0-9a-f]*$/i.test(body)) return null;
  const bytes = new Uint8Array(body.length / 2);
  for (let index = 0; index < bytes.length; index++) {
    bytes[index] = Number.parseInt(body.slice(index * 2, index * 2 + 2), 16);
  }
  return bytes;
}

function detect(bytes: Uint8Array): string | null {
  for (const [name, signature] of SIGNATURES) {
    if (signature.every((byte, index) => bytes[index] === byte)) return name;
  }
  const riff = String.fromCharCode(...bytes.slice(0, 4)) === "RIFF";
  if (riff && String.fromCharCode(...bytes.slice(8, 12)) === "WEBP") return "WebP image";
  return null;
}

function asUuid(bytes: Uint8Array): string {
  const hex = Array.from(bytes, byte => byte.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function hexByte(byte: number): string {
  return byte.toString(16).toUpperCase().padStart(2, "0");
}

function printable(byte: number): string {
  return byte >= 0x20 && byte < 0x7f ? String.fromCharCode(byte) : "·";
}

function BinaryView(props: { value: string }) {
  const bytes = createMemo(() => parseHex(props.value));

  const rows = createMemo(() => {
    const data = bytes();
    if (!data) return [];
    const count = Math.min(MAX_ROWS, Math.ceil(data.length / BYTES_PER_ROW));
    return Array.from({ length: count }, (_, row) => {
      const start = row * BYTES_PER_ROW;
      const slice = Array.from(data.subarray(start, start + BYTES_PER_ROW));
      const left = slice.slice(0, 8).map(hexByte).join(" ");
      const right = slice.slice(8).map(hexByte).join(" ");
      return {
        offset: start.toString(16).toUpperCase().padStart(8, "0"),
        hex: right ? `${left.padEnd(23)}  ${right}` : left,
        ascii: slice.map(printable).join(""),
      };
    });
  });

  const facts = createMemo<Fact[]>(() => {
    const data = bytes();
    if (!data) return [];
    const items: Fact[] = [{ label: "Size", value: `${data.length.toLocaleString()} bytes${data.length >= 1024 ? ` (${formatBytes(data.length)})` : ""}`, plain: true }];
    const kind = detect(data);
    if (kind) items.push({ label: "Looks like", value: kind, plain: true });
    if (data.length === 16) items.push({ label: "As UUID", value: asUuid(data) });
    return items;
  });

  const clipped = createMemo(() => (bytes()?.length ?? 0) > MAX_ROWS * BYTES_PER_ROW);

  return (
    <Show
      when={bytes()}
      fallback={<div class={`${styles.Note} ${styles.NoteProblem}`}>This isn't hex. Switch to Raw to fix it.</div>}
    >
      <div class={styles.BinaryView}>
        <Facts items={facts()} />
        <Show
          when={rows().length > 0}
          fallback={<div class={styles.EmptyBytes}>Empty value, 0 bytes</div>}
        >
          <div class={styles.HexDump}>
            <For each={rows()}>
              {row => (
                <div class={styles.HexRow}>
                  <span class={styles.Offset}>{row.offset}</span>
                  <span class={styles.HexBytes}>{row.hex}</span>
                  <span class={styles.Ascii}>{row.ascii}</span>
                </div>
              )}
            </For>
            <Show when={clipped()}>
              <div class={styles.Clipped}>
                Showing the first {formatBytes(MAX_ROWS * BYTES_PER_ROW)}. Switch to Raw to see all of it.
              </div>
            </Show>
          </div>
        </Show>
      </div>
    </Show>
  );
}

export default BinaryView;
