export type ManagerPasswordPolicyResult =
  | { valid: true; normalized: string }
  | { valid: false; reason: "tooShort" | "tooLong" };

export function validateManagerPassword(password: string): ManagerPasswordPolicyResult {
  const normalized = password.normalize("NFC");
  const codePoints = Array.from(normalized).length;
  if (codePoints < 15) return { valid: false, reason: "tooShort" };
  if (codePoints > 128) return { valid: false, reason: "tooLong" };
  return { valid: true, normalized };
}
