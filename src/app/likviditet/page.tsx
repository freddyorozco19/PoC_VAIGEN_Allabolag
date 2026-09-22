import { EXAMPLE_CASH_WEEKS, EXAMPLE_CASH_START_BALANCE } from "@/lib/mock/companies";
import { CashFlowChart } from "@/components/CashFlowChart";
import { AlertRow } from "@/components/AlertDot";
import { ExampleBadge } from "@/components/ExampleBadge";

export const metadata = { title: "Likviditetsprognos — Siffra" };

export default function LikviditetPage() {
  return (
    <div>
      <h2 className="mb-3 text-[1.7rem]">Likviditetsprognos, 13 veckor</h2>
      <div className="max-w-[640px] rounded-card border border-line bg-surface p-[14px]">
        <h4 className="mb-2 text-[0.78rem] font-semibold uppercase tracking-[0.08em] text-ink2">
          Förväntad kassa (tkr)
        </h4>
        <CashFlowChart weeks={EXAMPLE_CASH_WEEKS} startBalance={EXAMPLE_CASH_START_BALANCE} />
      </div>
      <div className="mt-3 max-w-[640px] rounded-card border border-line bg-surface p-[14px]">
        <AlertRow severity="warn">
          Momsinbetalning och löner infaller samma vecka som el mínimo de caja.
        </AlertRow>
      </div>
      <p className="mt-3 text-[0.78rem] text-ink2">
        <ExampleBadge /> Beräknat från kund- och leverantörsfakturor och återkommande betalningar.
        Fase 3 del plan de trabajo — requiere importación SIE.
      </p>
    </div>
  );
}
