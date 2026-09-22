import { EXAMPLE_INVOICES } from "@/lib/mock/companies";
import { ExampleBadge } from "@/components/ExampleBadge";

const STATUS_CLASSES: Record<string, string> = {
  good: "bg-good-soft text-good",
  warn: "bg-warn-soft text-warn",
  bad: "bg-bad-soft text-bad",
};

export const metadata = { title: "Kundfakturor — Siffra" };

export default function FakturorPage() {
  return (
    <div>
      <h2 className="mb-3 text-[1.7rem]">Kundfakturor</h2>
      <div className="overflow-x-auto rounded-card border border-line">
        <table className="w-full border-collapse text-[0.9rem]">
          <thead>
            <tr>
              <th className="border-b border-line px-[10px] py-[9px] text-left text-[0.75rem] uppercase tracking-[0.07em] text-ink2">
                Nr
              </th>
              <th className="border-b border-line px-[10px] py-[9px] text-left text-[0.75rem] uppercase tracking-[0.07em] text-ink2">
                Kund
              </th>
              <th className="border-b border-line px-[10px] py-[9px] text-right text-[0.75rem] uppercase tracking-[0.07em] text-ink2">
                Belopp (kr)
              </th>
              <th className="border-b border-line px-[10px] py-[9px] text-left text-[0.75rem] uppercase tracking-[0.07em] text-ink2">
                Förfaller
              </th>
              <th className="border-b border-line px-[10px] py-[9px] text-left text-[0.75rem] uppercase tracking-[0.07em] text-ink2">
                Status
              </th>
            </tr>
          </thead>
          <tbody>
            {EXAMPLE_INVOICES.map((inv) => (
              <tr key={inv.number}>
                <td className="border-b border-line px-[10px] py-[9px] font-mono last:border-0">
                  {inv.number}
                </td>
                <td className="border-b border-line px-[10px] py-[9px] last:border-0">{inv.customer}</td>
                <td className="border-b border-line px-[10px] py-[9px] text-right font-mono last:border-0">
                  {inv.amountSek.toLocaleString("sv-SE")}
                </td>
                <td className="border-b border-line px-[10px] py-[9px] last:border-0">{inv.dueDate}</td>
                <td className="border-b border-line px-[10px] py-[9px] last:border-0">
                  <span
                    className={`inline-block rounded-full px-[9px] py-[2px] text-[0.75rem] font-semibold ${STATUS_CLASSES[inv.severity]}`}
                  >
                    {inv.status}
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="mt-3 text-[0.78rem] text-ink2">
        <ExampleBadge /> Fase 3+ del plan de trabajo. El cliente podrá ser avisado si el pagador
        tiene riesgo elevado.
      </p>
    </div>
  );
}
