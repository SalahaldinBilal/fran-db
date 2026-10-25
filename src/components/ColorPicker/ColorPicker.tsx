import styles from "./ColorPicker.module.scss";
import { JSX, createMemo, createSignal, onMount } from "solid-js";
import { Portal } from "solid-js/web";
import { makeEventListener } from "@solid-primitives/event-listener";
import { keepInViewport } from "@core/helpers/viewport";
import Input from "@core/components/Input/Input";

export type ColorPickerProps = {
  anchor: HTMLElement,
  value: string,
  onChange: (hex: string) => void,
  onClose: () => void,
};

type Hsv = { h: number, s: number, v: number };

const HEX_PATTERN = /^#?([0-9a-f]{6})$/i;
const KEY_STEP = 0.02;

function clamp(value: number): number {
  return Math.min(Math.max(value, 0), 1);
}

function hsvToHex({ h, s, v }: Hsv): string {
  function channel(n: number) {
    const k = (n + h * 6) % 6;
    const value = v - v * s * Math.max(0, Math.min(k, 4 - k, 1));
    return Math.round(value * 255).toString(16).padStart(2, "0");
  }
  return `#${channel(5)}${channel(3)}${channel(1)}`;
}

function hexToHsv(hex: string): Hsv | null {
  const match = HEX_PATTERN.exec(hex.trim());
  if (!match) return null;

  const int = parseInt(match[1], 16);
  const [r, g, b] = [int >> 16, (int >> 8) & 255, int & 255].map(c => c / 255);
  const max = Math.max(r, g, b);
  const delta = max - Math.min(r, g, b);

  let h = 0;
  if (delta > 0) {
    if (max === r) h = ((g - b) / delta + 6) % 6;
    else if (max === g) h = (b - r) / delta + 2;
    else h = (r - g) / delta + 4;
  }
  return { h: h / 6, s: max === 0 ? 0 : delta / max, v: max };
}

function ColorPicker(props: ColorPickerProps) {
  // Kept as HSV so hue survives dragging to grey or black.
  const [hsv, setHsv] = createSignal<Hsv>(hexToHsv(props.value) ?? { h: 0, s: 0, v: 1 });
  const [hexText, setHexText] = createSignal(props.value);
  const hue = createMemo(() => hsvToHex({ h: hsv().h, s: 1, v: 1 }));
  const color = createMemo(() => hsvToHex(hsv()));
  const rect = props.anchor.getBoundingClientRect();
  let popover!: HTMLDivElement;

  onMount(() => {
    keepInViewport(popover);
    popover.focus();
  });

  function closeOutside(event: Event) {
    if (event.target instanceof Node && (popover.contains(event.target) || props.anchor.contains(event.target))) return;
    props.onClose();
  }

  // Placed against the viewport, so outside scrolling or resizing would detach it.
  makeEventListener(window, "pointerdown", closeOutside, { capture: true });
  makeEventListener(window, "scroll", closeOutside, { capture: true });
  makeEventListener(window, "resize", () => props.onClose());

  function commit(next: Hsv) {
    setHsv(next);
    const hex = hsvToHex(next);
    setHexText(hex);
    props.onChange(hex);
  }

  function dragHandler(apply: (x: number, y: number) => Hsv) {
    const update: JSX.EventHandler<HTMLDivElement, PointerEvent> = event => {
      const bounds = event.currentTarget.getBoundingClientRect();
      commit(apply(clamp((event.clientX - bounds.left) / bounds.width), clamp((event.clientY - bounds.top) / bounds.height)));
    };
    const onPointerDown: JSX.EventHandler<HTMLDivElement, PointerEvent> = event => {
      event.currentTarget.setPointerCapture(event.pointerId);
      update(event);
    };
    const onPointerMove: JSX.EventHandler<HTMLDivElement, PointerEvent> = event => {
      if (event.buttons & 1) update(event);
    };
    return { onPointerDown, onPointerMove };
  }

  const plane = dragHandler((x, y) => ({ h: hsv().h, s: x, v: 1 - y }));
  const hueTrack = dragHandler(x => ({ ...hsv(), h: Math.min(x, 0.9999) }));

  function onPlaneKey(event: KeyboardEvent) {
    const { h, s, v } = hsv();
    const moves: Record<string, Hsv> = {
      ArrowLeft: { h, s: clamp(s - KEY_STEP), v },
      ArrowRight: { h, s: clamp(s + KEY_STEP), v },
      ArrowUp: { h, s, v: clamp(v + KEY_STEP) },
      ArrowDown: { h, s, v: clamp(v - KEY_STEP) },
    };
    if (!moves[event.key]) return;
    event.preventDefault();
    commit(moves[event.key]);
  }

  function onHueKey(event: KeyboardEvent) {
    const step = event.key === "ArrowLeft" ? -KEY_STEP : event.key === "ArrowRight" ? KEY_STEP : 0;
    if (!step) return;
    event.preventDefault();
    commit({ ...hsv(), h: Math.min(clamp(hsv().h + step), 0.9999) });
  }

  function onHexInput(text: string) {
    setHexText(text);
    const parsed = hexToHsv(text);
    if (!parsed) return;
    setHsv(parsed);
    props.onChange(hsvToHex(parsed));
  }

  return (
    <Portal>
      <div
        ref={popover}
        class={styles.Popover}
        style={{ left: `${rect.left}px`, top: `${rect.bottom + 8}px`, "--hue": hue(), "--color": color() }}
        tabIndex={-1}
        role="dialog"
        aria-label="Custom color"
        onKeyDown={event => {
          if (event.key !== "Escape") return;
          event.stopPropagation();
          props.onClose();
          props.anchor.focus();
        }}
      >
        <div
          class={styles.Plane}
          tabIndex={0}
          role="slider"
          aria-label="Saturation and brightness"
          aria-valuetext={`Saturation ${Math.round(hsv().s * 100)}%, brightness ${Math.round(hsv().v * 100)}%`}
          onKeyDown={onPlaneKey}
          {...plane}
        >
          <span class={styles.Thumb} style={{ left: `${hsv().s * 100}%`, top: `${(1 - hsv().v) * 100}%` }} />
        </div>
        <div
          class={styles.Hue}
          tabIndex={0}
          role="slider"
          aria-label="Hue"
          aria-valuemin={0}
          aria-valuemax={360}
          aria-valuenow={Math.round(hsv().h * 360)}
          onKeyDown={onHueKey}
          {...hueTrack}
        >
          <span class={styles.Thumb} style={{ left: `${hsv().h * 100}%`, "background-color": hue() }} />
        </div>
        <div class={styles.Footer}>
          <span class={styles.Preview} />
          <Input value={hexText()} onValue={onHexInput} mono />
        </div>
      </div>
    </Portal>
  );
}

export default ColorPicker;
