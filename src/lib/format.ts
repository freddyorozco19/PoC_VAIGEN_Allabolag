/** Formatea miles de kronor (tkr) al estilo sueco, con signo menos correcto. */
export function formatTkr(n: number): string {
  const sign = n < 0 ? "−" : "";
  return sign + Math.abs(n).toLocaleString("sv-SE");
}

export function formatPercent(n: number, decimals = 1): string {
  return `${n.toFixed(decimals)} %`;
}
