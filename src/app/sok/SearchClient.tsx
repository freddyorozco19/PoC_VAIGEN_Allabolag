"use client";

import { useMemo, useState } from "react";
import Link from "next/link";
import { searchExampleCompanies } from "@/lib/mock/companies";
import { formatTkr } from "@/lib/format";
import { RiskPill } from "@/components/RiskPill";
import { ExampleBadge } from "@/components/ExampleBadge";

export function SearchClient() {
  const [query, setQuery] = useState("");
  const results = useMemo(() => searchExampleCompanies(query), [query]);

  return (
    <div>
      <div className="mb-4">
        <input
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Sök på namn, organisationsnummer eller ort"
          aria-label="Sök företag"
          className="w-full max-w-[420px] rounded-[6px] border border-line bg-bg px-3 py-[10px] text-ink placeholder:text-ink2"
        />
      </div>
      <div className="overflow-x-auto rounded-card border border-line">
        <table className="w-full border-collapse text-[0.9rem]">
          <thead>
            <tr>
              <Th>Företag</Th>
              <Th>Ort</Th>
              <Th align="right">Omsättning 2024 (tkr)</Th>
              <Th align="right">Resultat (tkr)</Th>
              <Th>Risk</Th>
            </tr>
          </thead>
          <tbody>
            {results.map((c) => (
              <tr key={c.id}>
                <Td>
                  <Link
                    href={`/foretag/${c.orgNumber}`}
                    className="font-semibold text-ink hover:text-accent hover:underline"
                  >
                    {c.name}
                  </Link>
                  <div className="mt-[3px] font-mono text-[0.78rem] text-ink2">{c.orgNumber}</div>
                </Td>
                <Td>{c.city}</Td>
                <Td align="right" mono>
                  {formatTkr(c.financials.revenue[4])}
                </Td>
                <Td align="right" mono>
                  {formatTkr(c.financials.result[4])}
                </Td>
                <Td>
                  <RiskPill level={c.riskLevel} />
                </Td>
              </tr>
            ))}
            {results.length === 0 ? (
              <tr>
                <td colSpan={5} className="px-[10px] py-[9px] text-ink2">
                  Inga träffar. Prova ett annat namn eller organisationsnummer.
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </div>
      <p className="mt-3 text-[0.78rem] text-ink2">
        <ExampleBadge /> Tres empresas ficticias. En producción esta lista viene de SCB y
        Bolagsverket.
      </p>
    </div>
  );
}

function Th({ children, align }: { children: React.ReactNode; align?: "right" }) {
  return (
    <th
      className={`border-b border-line px-[10px] py-[9px] text-[0.75rem] font-semibold uppercase tracking-[0.07em] text-ink2 ${
        align === "right" ? "text-right" : "text-left"
      }`}
    >
      {children}
    </th>
  );
}

function Td({
  children,
  align,
  mono,
}: {
  children: React.ReactNode;
  align?: "right";
  mono?: boolean;
}) {
  return (
    <td
      className={`border-b border-line px-[10px] py-[9px] align-top last:border-0 ${
        align === "right" ? "text-right" : "text-left"
      } ${mono ? "font-mono" : ""}`}
    >
      {children}
    </td>
  );
}
