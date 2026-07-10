import { Portal } from "solid-js/web";
import { Show, createSignal, createEffect } from "solid-js";
import type { JSX } from "solid-js";

interface Props {
  open: boolean;
  // Anchor point in viewport coordinates — typically the triggering click's
  // clientX/clientY, so the menu appears at the cursor (desktop-native).
  x: number;
  y: number;
  onClose: () => void;
  // Optional extra class on the panel; the shared `.context-menu` look always
  // applies (border, shadow, padding).
  panelClass?: string;
  children: JSX.Element;
}

const MARGIN = 8;

/**
 * A popover menu rendered in a Portal so it escapes the `overflow` clipping of
 * its trigger's ancestors. An in-tree `position: absolute` menu (see the old
 * `.context-menu`) is clipped by the messages scroll box and painted over by the
 * composer when its anchor is near the bottom. Portalling to <body> + `position:
 * fixed` sidesteps that; we then clamp the panel into the viewport (opening
 * upward / shifting left when it would run off an edge).
 */
export default function FloatingMenu(props: Props) {
  let panelRef: HTMLDivElement | undefined;
  const [pos, setPos] = createSignal<{ left: number; top: number }>({ left: 0, top: 0 });

  createEffect(() => {
    if (!props.open) return;
    const ax = props.x;
    const ay = props.y;
    // Paint at the anchor immediately, then measure + clamp on the next frame
    // (offsetWidth/Height are only meaningful once the panel is laid out).
    setPos({ left: ax, top: ay });
    requestAnimationFrame(() => {
      const el = panelRef;
      if (!el) return;
      const w = el.offsetWidth;
      const h = el.offsetHeight;
      const vw = window.innerWidth;
      const vh = window.innerHeight;
      let left = ax;
      let top = ay;
      if (left + w > vw - MARGIN) left = vw - w - MARGIN;
      if (left < MARGIN) left = MARGIN;
      // Flip above the anchor if opening downward would overflow the bottom.
      if (top + h > vh - MARGIN) top = ay - h;
      if (top < MARGIN) top = MARGIN;
      setPos({ left, top });
    });
  });

  return (
    <Show when={props.open}>
      <Portal>
        <div class="context-menu-backdrop" onClick={() => props.onClose()} />
        <div
          ref={panelRef}
          class={`context-menu${props.panelClass ? ` ${props.panelClass}` : ""}`}
          style={{
            position: "fixed",
            left: `${pos().left}px`,
            top: `${pos().top}px`,
            "margin-top": "0",
          }}
        >
          {props.children}
        </div>
      </Portal>
    </Show>
  );
}
