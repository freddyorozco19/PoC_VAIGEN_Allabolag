import { FINANCIAL_YEARS, type FiveYearSeries } from "@/lib/types";

/** Gráfico de barras SVG puro (sin dependencias) para 5 años de omsättning. */
export function RevenueChart({ revenue }: { revenue: FiveYearSeries }) {
  const W = 520;
  const H = 210;
  const pl = 44;
  const pb = 26;
  const pt = 16;
  const pr = 10;
  const max = Math.max(...revenue);
  const top = Math.ceil(max / 20000) * 20000 || 20000;
  const bw = (W - pl - pr) / 5;

  const gridLines = Array.from({ length: 5 }, (_, i) => {
    const value = (top / 4) * i;
    const y = H - pb - ((H - pb - pt) * value) / top;
    return { value, y };
  });

  return (
    <svg
      viewBox={`0 0 ${W} ${H}`}
      width="100%"
      role="img"
      aria-label="Omsättning, fem år"
      className="[&_text]:fill-ink2 [&_text]:font-mono [&_text]:text-[10px]"
    >
      {gridLines.map((g) => (
        <g key={g.value}>
          <line x1={pl} x2={W - pr} y1={g.y} y2={g.y} stroke="var(--line)" strokeWidth={1} />
          <text x={pl - 6} y={g.y + 3} textAnchor="end">
            {g.value / 1000} mkr
          </text>
        </g>
      ))}
      {revenue.map((v, i) => {
        const h = ((H - pb - pt) * v) / top;
        const x = pl + i * bw + bw * 0.2;
        const w = bw * 0.6;
        const y = H - pb - h;
        const isLast = i === revenue.length - 1;
        return (
          <g key={i}>
            <rect x={x} y={y} width={w} height={h} fill={isLast ? "var(--bar)" : "var(--bar2)"} />
            <text x={x + w / 2} y={H - 8} textAnchor="middle">
              {FINANCIAL_YEARS[i]}
            </text>
            {isLast ? (
              <text x={x + w / 2} y={y - 5} textAnchor="middle" fill="var(--ink)">
                {(v / 1000).toFixed(1)}
              </text>
            ) : null}
          </g>
        );
      })}
    </svg>
  );
}
