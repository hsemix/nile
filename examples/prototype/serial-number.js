// One native input, enhanced by CSS `prototype:`. Native selection, clipboard,
// focus and keyboard navigation all operate on the complete serial number.
const clean = value => String(value ?? '').toUpperCase().replace(/[^A-Z0-9]/g, '').slice(0, 24);
const format = value => (clean(value).match(/.{1,4}/g) || []).join('-');
const position = count => count ? count + Math.floor((count - 1) / 4) : 0;

export class SerialNumber extends Element {
  get value() { return super.value; }
  set value(value) { super.value = format(value); }
  get complete() { return /^[A-Z0-9]{4}(-[A-Z0-9]{4}){5}$/.test(this.value); }

  componentDidMount() { this.value = this.value; }

  ['on input']() {
    const text = this.value;
    const start = clean(text.slice(0, this.selectionStart)).length;
    const end = clean(text.slice(0, this.selectionEnd)).length;
    this.value = text;
    this.setSelectionRange(position(start), position(end));
  }

  ['on keydown'](event) {
    // Skip the separator when deleting so Backspace/Delete always remove a
    // character, rather than repeatedly removing an automatically inserted dash.
    if (event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return false;
    const at = this.selectionStart;
    if (at !== this.selectionEnd) return false;
    const text = this.value;
    let before;
    let after;
    if (event.key === 'Backspace' && at > 1 && text[at - 1] === '-') {
      before = text.slice(0, at - 2);
      after = text.slice(at);
    } else if (event.key === 'Delete' && text[at] === '-') {
      before = text.slice(0, at);
      after = text.slice(at + 2);
    } else return false;
    this.value = before + after;
    const caret = position(clean(before).length);
    this.setSelectionRange(caret, caret);
    this.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  }
}
