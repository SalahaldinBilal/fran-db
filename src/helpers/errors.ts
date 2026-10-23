import { AppError } from "@core/types";

export function isAppError(value: unknown): value is AppError {
  return typeof value === "object"
    && value !== null
    && "message" in value
    && "detail" in value
    && typeof (value as AppError).detail === "object";
}

export function errorMessage(error: unknown): string {
  if (isAppError(error)) return error.message;
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return String(error);
}
