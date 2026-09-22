import type { Company } from "@/lib/types";
import { formatTkr } from "@/lib/format";

/**
 * STUB — no llama a ningún modelo de IA todavía.
 *
 * Reproduce en código el texto de EJEMPLO del mockup original para que la
 * pestaña "Sammanfattning" funcione sin depender de un backend. Cuando
 * exista el módulo real (Fase 2, ver plan de 12 semanas), esta función se
 * sustituye por una llamada al servicio de resumen que cite las cifras
 * reales extraídas del iXBRL.
 */
export function generateExampleSummary(company: Company): string {
  const { revenue, result, equity, totalAssets } = company.financials;
  const growth = (revenue[4] / revenue[3] - 1) * 100;
  const margin = (result[4] / revenue[4]) * 100;
  const solidity = (equity[4] / totalAssets[4]) * 100;

  if (company.riskLevel === "bad") {
    return `Omsättningen har fallit tre år i rad och bolaget redovisar förlust de två senaste åren (${formatTkr(
      result[3]
    )} och ${formatTkr(result[4])} tkr). Soliditeten är ${solidity.toFixed(
      1
    )} %, under branschens median. Eget kapital har mer än halverats sedan 2022. Kräver närmare granskning innan kreditgivning.`;
  }
  if (company.riskLevel === "warn") {
    return `Stabil men liten verksamhet. Omsättningen växer (${growth.toFixed(
      1
    )} % senaste året), men vinstmarginalen på ${margin.toFixed(
      1
    )} % ligger under branschens median. Soliditeten är god.`;
  }
  return `Stark utveckling. Omsättningen har vuxit varje år, till ${formatTkr(
    revenue[4]
  )} tkr, och soliditeten är ${solidity.toFixed(1)} %, över branschens median. Inga varningssignaler i underlaget.`;
}
