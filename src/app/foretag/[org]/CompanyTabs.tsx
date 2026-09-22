"use client";

import { useState } from "react";
import type { Company } from "@/lib/types";
import { FINANCIAL_YEARS } from "@/lib/types";
import { formatTkr } from "@/lib/format";
import { RevenueChart } from "@/components/RevenueChart";
import { BenchmarkBar } from "@/components/BenchmarkBar";
import { AlertRow } from "@/components/AlertDot";
import { ExampleBadge } from "@/components/ExampleBadge";
import { generateExampleSummary } from "@/lib/ai-summary";

const SUB_TABS = [
  { id: "ov", label: "Översikt" },
  { id: "fin", label: "Bokslut" },
  { id: "ppl", label: "Personer" },
  { id: "ai", label: "Sammanfattning" },
] as const;

type SubTabId = (typeof SUB_TABS)[number]["id"];

export function CompanyTabs({ company }: { company: Company }) {
  const [sub, setSub] = useState<SubTabId>("ov");

  return (
    <div>
      <div role="tablist" className="mb-4 flex gap-[2px] overflow-x-auto border-b border-line">
        {SUB_TABS.map((t) => (
          <button
            key={t.id}
            role="tab"
            aria-selected={sub === t.id}
            onClick={() => setSub(t.id)}
            className={`whitespace-nowrap border-b-2 px-3 py-2 ${
              sub === t.id
                ? "border-accent font-semibold text-ink"
                : "border-transparent text-ink2 hover:text-ink"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      {sub === "ov" && <Overview company={company} />}
      {sub === "fin" && <Financials company={company} />}
      {sub === "ppl" && <People company={company} />}
      {sub === "ai" && <Summary company={company} />}
    </div>
  );
}

function Overview({ company }: { company: Company }) {
  const sniCode = company.sni.slice(0, 6);
  return (
    <div>
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-[1.4fr_1fr]">
        <div className="rounded-card border border-line bg-surface p-[14px]">
          <h4 className="mb-2 text-[0.78rem] font-semibold uppercase tracking-[0.08em] text-ink2">
            Omsättning, 5 år (tkr)
          </h4>
          <RevenueChart revenue={company.financials.revenue} />
        </div>
        <div className="rounded-card border border-line bg-surface p-[14px]">
          <h4 className="mb-2 text-[0.78rem] font-semibold uppercase tracking-[0.08em] text-ink2">
            Mot branschen (SNI-median)
          </h4>
          <BenchmarkBar label="Vinstmarginal" pair={company.benchmarks.margin} scaleMax={30} />
          <BenchmarkBar label="Soliditet" pair={company.benchmarks.solidity} scaleMax={80} />
          <BenchmarkBar label="Kassalikviditet" pair={company.benchmarks.liquidity} scaleMax={250} />
          <p className="mt-[10px] text-[0.78rem] text-ink2">Strecket visar medianen för SNI {sniCode}.</p>
        </div>
      </div>

      <div className="mt-4 rounded-card border border-line bg-surface p-[14px]">
        <h4 className="mb-2 text-[0.78rem] font-semibold uppercase tracking-[0.08em] text-ink2">
          Signaler
        </h4>
        {company.alerts.map((a, i) => (
          <AlertRow key={i} severity={a.severity}>
            {a.text}
          </AlertRow>
        ))}
      </div>
      <p className="mt-3 text-[0.78rem] text-ink2">Verksamhet: {company.sni}</p>
    </div>
  );
}

function Financials({ company }: { company: Company }) {
  const rows: [string, readonly number[]][] = [
    ["Omsättning", company.financials.revenue],
    ["Resultat efter finansiella poster", company.financials.result],
    ["Eget kapital", company.financials.equity],
    ["Summa tillgångar", company.financials.totalAssets],
  ];

  return (
    <div>
      <div className="overflow-x-auto rounded-card border border-line">
        <table className="w-full border-collapse text-[0.9rem]">
          <thead>
            <tr>
              <th className="border-b border-line px-[10px] py-[9px] text-left text-[0.75rem] uppercase tracking-[0.07em] text-ink2">
                tkr
              </th>
              {FINANCIAL_YEARS.map((y) => (
                <th
                  key={y}
                  className="border-b border-line px-[10px] py-[9px] text-right text-[0.75rem] uppercase tracking-[0.07em] text-ink2"
                >
                  {y}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map(([label, values]) => (
              <tr key={label}>
                <td className="border-b border-line px-[10px] py-[9px] last:border-0">{label}</td>
                {values.map((v, i) => (
                  <td
                    key={i}
                    className={`border-b border-line px-[10px] py-[9px] text-right font-mono last:border-0 ${
                      v < 0 ? "text-bad" : ""
                    }`}
                  >
                    {formatTkr(v)}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="mt-3 text-[0.78rem] text-ink2">
        Källa: digitalt inlämnade årsredovisningar (iXBRL), Bolagsverket. <ExampleBadge />
      </p>
      <button disabled className="mt-2 cursor-not-allowed rounded-full border border-line bg-surface px-[11px] py-[6px] text-[0.82rem] text-ink2">
        Ladda ner årsredovisning (zip)
      </button>
    </div>
  );
}

function People({ company }: { company: Company }) {
  return (
    <div>
      <div className="overflow-x-auto rounded-card border border-line">
        <table className="w-full border-collapse text-[0.9rem]">
          <thead>
            <tr>
              <th className="border-b border-line px-[10px] py-[9px] text-left text-[0.75rem] uppercase tracking-[0.07em] text-ink2">
                Roll
              </th>
              <th className="border-b border-line px-[10px] py-[9px] text-left text-[0.75rem] uppercase tracking-[0.07em] text-ink2">
                Namn
              </th>
            </tr>
          </thead>
          <tbody>
            {company.people.map((p) => (
              <tr key={p.role + p.name}>
                <td className="border-b border-line px-[10px] py-[9px] last:border-0">{p.role}</td>
                <td className="border-b border-line px-[10px] py-[9px] last:border-0">{p.name}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="mt-[10px]">
        Firmateckning: <strong>{company.firmateckning}</strong>
      </p>
      <p className="mt-2 text-[0.78rem] text-ink2">
        Kräver Bolagsverkets betalda API (fas 2). Namn är påhittade. <ExampleBadge />
      </p>
    </div>
  );
}

function Summary({ company }: { company: Company }) {
  return (
    <div className="rounded-card border border-line bg-surface p-[14px]">
      <h4 className="mb-2 text-[0.78rem] font-semibold uppercase tracking-[0.08em] text-ink2">
        Automatisk sammanfattning
      </h4>
      <p className="max-w-[68ch]">{generateExampleSummary(company)}</p>
      <p className="mt-2 text-[0.78rem] text-ink2">
        Genererad från siffrorna på fliken Bokslut. Ingen kreditbedömning. <ExampleBadge />
      </p>
    </div>
  );
}
