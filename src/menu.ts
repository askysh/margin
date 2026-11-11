import { useEffect, useId, useState } from "react";

const MENU_OPENED = "margin-menu-opened";

/** Open state for a drop-down menu. A click anywhere or Escape closes it, and so does
 * another menu that opens. The toggle buttons stop their click, so without the last rule
 * two menus could be open at once. */
export function useMenu() {
  const [open, setOpen] = useState(false);
  const id = useId();
  useEffect(() => {
    if (!open) return;
    window.dispatchEvent(new CustomEvent(MENU_OPENED, { detail: id }));
    const close = () => setOpen(false);
    const other = (e: Event) => (e as CustomEvent<string>).detail !== id && close();
    const esc = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      // Escape closes the menu only, not the page or panel behind it.
      e.preventDefault();
      close();
    };
    window.addEventListener("click", close);
    window.addEventListener("keydown", esc);
    window.addEventListener(MENU_OPENED, other);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("keydown", esc);
      window.removeEventListener(MENU_OPENED, other);
    };
  }, [open, id]);
  return [open, setOpen] as const;
}
