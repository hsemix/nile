// A class assigned from CSS (`prototype: StepperBox url(stepper.js)`). Its methods named "on ..." are
// event handlers: `on <event> [at <selector>]`; returning true marks the event handled.
export class StepperBox extends Element {
  count = 0;

  componentDidMount() {
    this.min = Number(this.getAttribute("min") ?? -Infinity);
    this.max = Number(this.getAttribute("max") ?? Infinity);
    this.count = Number(this.getAttribute("value") ?? 0);
    this.state.focusable = true;
    this.content(<>
      <button class="dec">-</button><b>{this.count}</b><button class="inc">+</button>
    </>);
  }

  step(by) {
    const next = Math.min(this.max, Math.max(this.min, this.count + by));
    if (next === this.count) return true;
    this.count = next;
    this.$("b").textContent = next;
    this.setAttribute("value", next);
    this.dispatchEvent(new Event("change", { bubbles: true }));
    return true;
  }

  ["on click at button.inc"]() { return this.step(1); }
  ["on click at button.dec"]() { return this.step(-1); }

  ["on keydown"](event) {
    if (event.key === "ArrowUp" || event.key === "ArrowRight") return this.step(1);
    if (event.key === "ArrowDown" || event.key === "ArrowLeft") return this.step(-1);
    return false;
  }
}
