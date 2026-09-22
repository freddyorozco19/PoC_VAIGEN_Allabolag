import type { RiskLevel } from "@/lib/types";

const RISK_LABELS: Record<RiskLevel, string> = {
  good: "Låg risk",
  warn: "Bevaka",
  bad: "Förhöjd risk",
};

const RISK_CLASSES: Record<RiskLevel, string> = {
  good: "bg-good-soft text-good",
  warn: "bg-warn-soft text-warn",
  bad: "bg-bad-soft text-bad",
};

export function RiskPill({ level }: { level: RiskLevel }) {
  return (
    <span
      className={`inline-block rounded-full px-[9px] py-[2px] text-[0.75rem] font-semibold tracking-wide ${RISK_CLASSES[level]}`}
    >
      {RISK_LABELS[level]}
    </span>
  );
}

export function StatusPill({ children }: { children: React.ReactNode }) {
  return (
    <span className="inline-block rounded-full bg-good-soft px-[9px] py-[2px] text-[0.75rem] font-semibold text-good">
      {children}
    </span>
  );
}
