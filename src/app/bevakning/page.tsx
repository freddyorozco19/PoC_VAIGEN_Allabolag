import { EXAMPLE_WATCH_ALERTS } from "@/lib/mock/companies";
import { AlertRow } from "@/components/AlertDot";
import { ExampleBadge } from "@/components/ExampleBadge";

export const metadata = { title: "Bevakning — Siffra" };

export default function BevakningPage() {
  return (
    <div>
      <h2 className="mb-3 text-[1.7rem]">Bevakning</h2>
      <div className="max-w-[640px] rounded-card border border-line bg-surface p-[14px]">
        {EXAMPLE_WATCH_ALERTS.map((a, i) => (
          <AlertRow key={i} severity={a.severity}>
            <strong>{a.companyName}</strong>
            <br />
            {a.text} <span className="text-ink2">· {a.when}</span>
          </AlertRow>
        ))}
      </div>
      <p className="mt-3 text-[0.78rem] text-ink2">
        <ExampleBadge /> Mail varje måndag med det som ändrats. Fase 2 del plan de trabajo —
        depende de notificaciones de Bolagsverket (API de pago) o comparación semanal del archivo
        gratuito.
      </p>
    </div>
  );
}
