import { notFound } from "next/navigation";
import Link from "next/link";
import { EXAMPLE_COMPANIES, findExampleCompany } from "@/lib/mock/companies";
import { formatTkr } from "@/lib/format";
import { RiskPill, StatusPill } from "@/components/RiskPill";
import { CompanyTabs } from "./CompanyTabs";

export function generateStaticParams() {
  return EXAMPLE_COMPANIES.map((c) => ({ org: c.orgNumber }));
}

export default async function CompanyProfilePage({
  params,
}: {
  params: Promise<{ org: string }>;
}) {
  const { org } = await params;
  const company = findExampleCompany(org);
  if (!company) notFound();

  const growth = (company.financials.revenue[4] / company.financials.revenue[3] - 1) * 100;
  const margin = (company.financials.result[4] / company.financials.revenue[4]) * 100;
  const solidity = (company.financials.equity[4] / company.financials.totalAssets[4]) * 100;

  return (
    <div>
      <Link href="/sok" className="text-sm text-ink2 hover:text-accent">
        ← Sökresultat
      </Link>

      <div className="my-[6px] mb-[14px] flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 className="text-[1.5rem]">{company.name}</h3>
          <div className="font-mono text-[0.82rem] text-ink2">
            Org.nr {company.orgNumber} · {company.legalForm} · {company.city}
          </div>
        </div>
        <div className="flex flex-wrap gap-[6px]">
          <StatusPill>{company.status}</StatusPill>
          <RiskPill level={company.riskLevel} />
        </div>
      </div>

      <div className="mb-4 grid grid-cols-1 gap-px overflow-hidden rounded-card border border-line bg-line sm:grid-cols-2 lg:grid-cols-4">
        <Kpi
          eyebrow="Omsättning"
          value={`${formatTkr(company.financials.revenue[4])} tkr`}
          detail={`${growth >= 0 ? "▲" : "▼"} ${Math.abs(growth).toFixed(1)} % mot 2023`}
          detailColor={growth >= 0 ? "text-good" : "text-bad"}
        />
        <Kpi
          eyebrow="Resultat"
          value={`${formatTkr(company.financials.result[4])} tkr`}
          detail={`Vinstmarginal ${margin.toFixed(1)} %`}
        />
        <Kpi
          eyebrow="Soliditet"
          value={`${solidity.toFixed(1)} %`}
          detail={`Eget kapital ${formatTkr(company.financials.equity[4])} tkr`}
        />
        <Kpi eyebrow="Anställda" value={company.employeeRange} detail="SCB, intervall" />
      </div>

      <CompanyTabs company={company} />
    </div>
  );
}

function Kpi({
  eyebrow,
  value,
  detail,
  detailColor = "text-ink2",
}: {
  eyebrow: string;
  value: string;
  detail: string;
  detailColor?: string;
}) {
  return (
    <div className="bg-surface p-[12px_14px]">
      <div className="font-mono text-[0.72rem] uppercase tracking-[0.09em] text-ink2">{eyebrow}</div>
      <div className="font-display text-[1.45rem] font-bold">{value}</div>
      <div className={`text-[0.78rem] ${detailColor}`}>{detail}</div>
    </div>
  );
}
