export interface ActivateOptions {
  onActivate?: (event: KeyboardEvent) => void;
}

export function attachActivate(
  node: HTMLElement,
  options?: ActivateOptions,
): () => void {
  function handle(event: KeyboardEvent): void {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    if (options?.onActivate) {
      options.onActivate(event);
    } else {
      node.click();
    }
  }
  node.addEventListener("keydown", handle);
  return () => node.removeEventListener("keydown", handle);
}

export function attachAsButton(
  node: HTMLElement,
  options?: ActivateOptions,
): () => void {
  if (!node.hasAttribute("role")) node.setAttribute("role", "button");
  if (!node.hasAttribute("tabindex")) node.setAttribute("tabindex", "0");
  return attachActivate(node, options);
}
