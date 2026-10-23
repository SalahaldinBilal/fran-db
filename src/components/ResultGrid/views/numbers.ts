export type IntegerInfo = { min: bigint, max: bigint, bits: number | null, zeroAllowed: boolean };

const INTEGER_BITS: Record<string, number> = { TINYINT: 8, SMALLINT: 16, MEDIUMINT: 24, INT: 32, INTEGER: 32, BIGINT: 64 };

const INTEGER_PATTERN = /^[+-]?\d+$/;
const DECIMAL_PATTERN = /^[+-]?(\d+\.?\d*|\.\d+)$/;
const FLOAT_PATTERN = /^[+-]?(\d+\.?\d*|\.\d+)(e[+-]?\d+)?$/i;

export function integerInfo(typeName: string): IntegerInfo | null {
  const [base, modifier] = typeName.toUpperCase().split(" ");
  if (base === "YEAR") return { min: 1901n, max: 2155n, bits: null, zeroAllowed: true };

  const bits = INTEGER_BITS[base];
  if (!bits) return null;
  const width = BigInt(bits);
  return modifier === "UNSIGNED"
    ? { min: 0n, max: (1n << width) - 1n, bits, zeroAllowed: false }
    : { min: -(1n << (width - 1n)), max: (1n << (width - 1n)) - 1n, bits, zeroAllowed: false };
}

export function isInteger(text: string): boolean {
  return INTEGER_PATTERN.test(text);
}

export function isDecimal(text: string): boolean {
  return DECIMAL_PATTERN.test(text);
}

export function isFloat(text: string): boolean {
  return FLOAT_PATTERN.test(text);
}

export function inRange(value: bigint, info: IntegerInfo): boolean {
  return (value >= info.min && value <= info.max) || (info.zeroAllowed && value === 0n);
}

export function clamp(value: bigint, info: IntegerInfo): bigint {
  return value < info.min ? info.min : value > info.max ? info.max : value;
}

const decimalSeparator = new Intl.NumberFormat().formatToParts(1.5).find(part => part.type === "decimal")?.value ?? ".";

// Works on the text itself so DECIMAL(65,30) values keep every digit.
export function groupDigits(text: string): string {
  const negative = text.startsWith("-");
  const unsigned = text.replace(/^[+-]/, "");
  const [whole, fraction] = unsigned.split(".");
  const grouped = BigInt(whole || "0").toLocaleString();
  return `${negative ? "-" : ""}${grouped}${fraction !== undefined ? `${decimalSeparator}${fraction}` : ""}`;
}

export function scientific(text: string): string {
  const value = Number(text);
  return Number.isFinite(value) ? value.toExponential() : "";
}

export function digitCounts(text: string): { whole: number, fraction: number } {
  const [whole, fraction = ""] = text.replace(/^[+-]/, "").split(".");
  return { whole: whole.replace(/^0+(?=\d)/, "").length, fraction: fraction.length };
}

function unsignedForm(value: bigint, bits: number | null): bigint | null {
  if (value >= 0n) return value;
  return bits === null ? null : (1n << BigInt(bits)) + value;
}

export function toHex(value: bigint, bits: number | null): string | null {
  const unsigned = unsignedForm(value, bits);
  if (unsigned === null) return null;
  const digits = unsigned.toString(16).toUpperCase();
  const width = value < 0n && bits !== null ? bits / 4 : Math.ceil(digits.length / 2) * 2;
  return `0x${digits.padStart(width, "0")}`;
}

export function toBinary(value: bigint, bits: number | null): string | null {
  const unsigned = unsignedForm(value, bits);
  if (unsigned === null) return null;
  const digits = unsigned.toString(2);
  const width = value < 0n && bits !== null ? bits : Math.ceil(digits.length / 8) * 8;
  return digits.padStart(width, "0").replace(/(.{4})(?=.)/g, "$1 ");
}

export function bitWidth(value: bigint): number {
  const used = value > 0n ? value.toString(2).length : 1;
  return Math.min(64, Math.max(8, Math.ceil(used / 8) * 8));
}

export function countBits(value: bigint): number {
  return [...value.toString(2)].filter(digit => digit === "1").length;
}
