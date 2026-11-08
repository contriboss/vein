/**
 * ```ts
 * @decorator()
 * fenced()
 * ```
 * Runs after the example.
 */
export declare function fenced(): void;
/**
 * @deprecated use `fenced`
 * (timestamps leak through)
 */
export declare const old: number;
/**
  /**
   * Opened twice, as date-fns 4 writes `ContextFn`'s comment.
   */
export type Twice = number;
