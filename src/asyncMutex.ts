/** Serialize async critical sections so concurrent callers run one at a time. */
export function createAsyncMutex() {
  let tail: Promise<unknown> = Promise.resolve()

  return function runExclusive<T>(fn: () => Promise<T>): Promise<T> {
    const run = tail.then(fn, fn)
    tail = run.then(
      () => undefined,
      () => undefined,
    )
    return run
  }
}
