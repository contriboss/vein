/** Parses anything. */
export declare function parse(input: unknown): unknown;
export declare class ZodError extends Error {}
export interface ZodType {
    _type: unknown;
}
export type Infer<T> = T;
export declare const string: () => ZodType, number: () => ZodType;
declare function object(): ZodType;
export { object, object as obj };
export * from "../shared";
