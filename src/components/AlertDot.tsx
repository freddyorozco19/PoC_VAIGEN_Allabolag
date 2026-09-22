import type { AlertSeverity } from "@/lib/types";

const DOT_CLASSES: Record<AlertSeverity, string> = {
  good: "bg-good",
  warn: "bg-warn",
  bad: "bg-bad",
};

export function AlertRow({ severity, children }: { severity: AlertSeverity; children: React.ReactNode }) {
  return (
    <div className="flex gap-[10px] border-b border-line py-[9px] text-[0.87rem] last:border-0">
      <span className={`mt-[6px] h-[9px] w-[9px] flex-none rounded-full ${DOT_CLASSES[severity]}`} />
      <span>{children}</span>
    </div>
  );
}
