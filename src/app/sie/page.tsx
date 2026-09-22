import { EXAMPLE_SIE_PREVIEW } from "@/lib/mock/companies";
import { ExampleBadge } from "@/components/ExampleBadge";

export const metadata = { title: "Importera SIE — Siffra" };

export default function SiePage() {
  return (
    <div>
      <h2 className="mb-3 text-[1.7rem]">Importera bokföring (SIE)</h2>
      <div className="max-w-[640px] rounded-card border border-dashed border-line bg-surface p-7 text-center">
        <strong>Släpp en SIE-fil här</strong>
        <p className="mx-auto mb-3 mt-[6px] max-w-[46ch] text-ink2">
          SIE 4 från Fortnox, Visma, Bokio eller annat program.
        </p>
        <button
          disabled
          className="cursor-not-allowed rounded-full border border-line bg-surface px-[11px] py-[6px] text-[0.82rem] text-ink2"
        >
          Välj fil
        </button>
      </div>

      <div className="mt-3 max-w-[640px] rounded-card border border-line bg-surface p-[14px]">
        <h4 className="mb-2 text-[0.78rem] font-semibold uppercase tracking-[0.08em] text-ink2">
          Förhandsvisning
        </h4>
        <div className="overflow-x-auto">
          <table className="w-full border-collapse text-[0.9rem]">
            <thead>
              <tr>
                <th className="border-b border-line px-[10px] py-[9px] text-left text-[0.75rem] uppercase tracking-[0.07em] text-ink2">
                  Konto (BAS)
                </th>
                <th className="border-b border-line px-[10px] py-[9px] text-left text-[0.75rem] uppercase tracking-[0.07em] text-ink2">
                  Namn
                </th>
                <th className="border-b border-line px-[10px] py-[9px] text-right text-[0.75rem] uppercase tracking-[0.07em] text-ink2">
                  Saldo
                </th>
              </tr>
            </thead>
            <tbody>
              {EXAMPLE_SIE_PREVIEW.map((row) => (
                <tr key={row.account}>
                  <td className="border-b border-line px-[10px] py-[9px] font-mono last:border-0">
                    {row.account}
                  </td>
                  <td className="border-b border-line px-[10px] py-[9px] last:border-0">{row.name}</td>
                  <td className="border-b border-line px-[10px] py-[9px] text-right font-mono last:border-0">
                    {row.balanceSek.toLocaleString("sv-SE")}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
      <p className="mt-3 text-[0.78rem] text-ink2">
        <ExampleBadge /> No hay parser SIE real todavía (Fase 3). Este es solo el layout de la
        pantalla.
      </p>
    </div>
  );
}
