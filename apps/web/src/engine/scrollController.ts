type ScrollFn = (x: number) => void;
type IdleFn = () => void;

let _x = 0;
let _isScrolling = false;
let _idleTimer: ReturnType<typeof setTimeout> | null = null;
const _scrollSubs = new Set<ScrollFn>();
const _idleSubs = new Set<IdleFn>();
const SCROLL_IDLE_MS = 100;

export function notifyScroll(x: number): void {
  _x = x;
  _isScrolling = true;
  for (const fn of _scrollSubs) fn(x);
  if (_idleTimer !== null) clearTimeout(_idleTimer);
  _idleTimer = setTimeout(() => {
    _idleTimer = null;
    _isScrolling = false;
    for (const fn of _idleSubs) fn();
  }, SCROLL_IDLE_MS);
}

export function subscribeScroll(fn: ScrollFn): () => void {
  _scrollSubs.add(fn);
  return () => _scrollSubs.delete(fn);
}

export function subscribeScrollIdle(fn: IdleFn): () => void {
  _idleSubs.add(fn);
  return () => _idleSubs.delete(fn);
}

export function isScrollingNow(): boolean {
  return _isScrolling;
}

export function getScrollX(): number {
  return _x;
}
