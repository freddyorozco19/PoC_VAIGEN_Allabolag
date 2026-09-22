/**
 * DATOS DE EJEMPLO (EJEMPLO/DEMO)
 *
 * Estas tres empresas son ficticias. Se usan únicamente para navegar el
 * mockup del producto. Los datos se copiaron tal cual del prototipo
 * interactivo validado (Claude Artifact "Siffra MVP") para no perder
 * el diseño ya revisado.
 *
 * Cuando exista la ingesta real (Bolagsverket + SCB, ver
 * src/lib/data-sources/), este fichero deja de usarse en producción,
 * pero puede seguir sirviendo como fixture para tests o para el modo
 * demo de ventas.
 */
import type { Company, WatchAlert, CashWeek, Invoice, SieAccountPreview } from "@/lib/types";

export const EXAMPLE_COMPANIES: Company[] = [
  {
    id: "a",
    name: "Nordlys Logistik AB",
    orgNumber: "559012-3456",
    legalForm: "Aktiebolag",
    sni: "52.290 Övriga stödtjänster till transport",
    city: "Göteborg",
    employeeRange: "10–19",
    status: "Aktiv",
    riskLevel: "good",
    firmateckning: "Två i förening",
    people: [
      { role: "Verkställande direktör", name: "Anna Testsson" },
      { role: "Styrelseordförande", name: "Bo Exempelsson" },
      { role: "Ledamot", name: "Cilla Provdotter" },
    ],
    alerts: [
      { severity: "good", text: "Nya årsredovisningen registrerades 12 juni" },
      { severity: "warn", text: "Styrelseledamot bytt i mars" },
    ],
    financials: {
      revenue: [41200, 45800, 52300, 58900, 64100],
      result: [1900, 2600, 3100, 2800, 4200],
      equity: [6100, 7900, 9800, 11200, 13900],
      totalAssets: [14200, 16800, 19500, 21300, 24100],
    },
    benchmarks: {
      margin: { value: 6.5, median: 5.1 },
      solidity: { value: 57.7, median: 32 },
      liquidity: { value: 168, median: 120 },
    },
  },
  {
    id: "b",
    name: "Fjällbruk Bygg & Design AB",
    orgNumber: "559108-7721",
    legalForm: "Aktiebolag",
    sni: "41.200 Byggande av bostadshus och andra byggnader",
    city: "Östersund",
    employeeRange: "20–49",
    status: "Aktiv",
    riskLevel: "bad",
    firmateckning: "Var för sig",
    people: [
      { role: "Verkställande direktör", name: "Dan Exempelson" },
      { role: "Styrelseordförande", name: "Eva Testlund" },
    ],
    alerts: [
      { severity: "bad", text: "Förlust två år i rad" },
      { severity: "bad", text: "Eget kapital under halva aktiekapitalet" },
      { severity: "warn", text: "Ny adress registrerad i april" },
    ],
    financials: {
      revenue: [88300, 91200, 84500, 79800, 72400],
      result: [4700, 3900, 1200, -2100, -3800],
      equity: [12800, 14200, 13100, 9700, 5300],
      totalAssets: [38100, 40200, 39800, 37500, 35100],
    },
    benchmarks: {
      margin: { value: -5.2, median: 4.4 },
      solidity: { value: 15.1, median: 28 },
      liquidity: { value: 71, median: 115 },
    },
  },
  {
    id: "c",
    name: "Kvarn & Krydda Livs AB",
    orgNumber: "559234-9905",
    legalForm: "Aktiebolag",
    sni: "47.290 Övrig specialiserad butikshandel med livsmedel",
    city: "Uppsala",
    employeeRange: "5–9",
    status: "Aktiv",
    riskLevel: "warn",
    firmateckning: "Var för sig",
    people: [
      { role: "Verkställande direktör", name: "Fredrik Provsson" },
      { role: "Styrelseordförande", name: "Gun Testström" },
    ],
    alerts: [
      { severity: "warn", text: "Marginalen är under sektorns median" },
      { severity: "good", text: "Inga anmärkningar registrerade i ejemplo" },
    ],
    financials: {
      revenue: [9800, 10400, 11900, 12100, 13300],
      result: [310, 280, 460, 150, 390],
      equity: [900, 1120, 1480, 1550, 1800],
      totalAssets: [3400, 3600, 4100, 4300, 4700],
    },
    benchmarks: {
      margin: { value: 2.9, median: 3.6 },
      solidity: { value: 38.3, median: 30 },
      liquidity: { value: 96, median: 105 },
    },
  },
];

export function findExampleCompany(idOrOrgNumber: string): Company | undefined {
  return EXAMPLE_COMPANIES.find(
    (c) => c.id === idOrOrgNumber || c.orgNumber === idOrOrgNumber
  );
}

export function searchExampleCompanies(query: string): Company[] {
  const q = query.trim().toLowerCase();
  if (!q) return EXAMPLE_COMPANIES;
  return EXAMPLE_COMPANIES.filter(
    (c) =>
      c.name.toLowerCase().includes(q) ||
      c.orgNumber.includes(q) ||
      c.city.toLowerCase().includes(q)
  );
}

export const EXAMPLE_WATCH_ALERTS: WatchAlert[] = [
  {
    severity: "bad",
    companyName: "Fjällbruk Bygg & Design AB",
    text: "Förlust två år i rad registrerad",
    when: "idag",
  },
  {
    severity: "warn",
    companyName: "Kvarn & Krydda Livs AB",
    text: "Ny adress registrerad",
    when: "för 3 dagar sedan",
  },
  {
    severity: "good",
    companyName: "Nordlys Logistik AB",
    text: "Ny årsredovisning finns",
    when: "för 1 vecka sedan",
  },
];

/** Entradas y salidas de caja de EJEMPLO para las 13 semanas de la previsión de liquidez. */
export const EXAMPLE_CASH_WEEKS: CashWeek[] = [
  { week: 1, inflow: 180, outflow: 140 },
  { week: 2, inflow: 90, outflow: 210 },
  { week: 3, inflow: 0, outflow: 60 },
  { week: 4, inflow: 260, outflow: 120 },
  { week: 5, inflow: 120, outflow: 240 },
  { week: 6, inflow: 0, outflow: 80 },
  { week: 7, inflow: 340, outflow: 110 },
  { week: 8, inflow: 60, outflow: 260 },
  { week: 9, inflow: 210, outflow: 90 },
  { week: 10, inflow: 0, outflow: 180 },
  { week: 11, inflow: 150, outflow: 70 },
  { week: 12, inflow: 90, outflow: 230 },
  { week: 13, inflow: 300, outflow: 100 },
];

export const EXAMPLE_CASH_START_BALANCE = 420;

export const EXAMPLE_INVOICES: Invoice[] = [
  {
    number: "2024-118",
    customer: "Hamnkraft Test AB",
    amountSek: 48500,
    dueDate: "2025-02-14",
    status: "Förfallen",
    severity: "bad",
  },
  {
    number: "2024-121",
    customer: "Sundsvall Demo AB",
    amountSek: 22000,
    dueDate: "2025-03-02",
    status: "Obetald",
    severity: "warn",
  },
  {
    number: "2025-004",
    customer: "Lindqvist Prov AB",
    amountSek: 75300,
    dueDate: "2025-03-20",
    status: "Betald",
    severity: "good",
  },
];

export const EXAMPLE_SIE_PREVIEW: SieAccountPreview[] = [
  { account: "1930", name: "Företagskonto", balanceSek: 412300 },
  { account: "1510", name: "Kundfordringar", balanceSek: 286500 },
  { account: "2440", name: "Leverantörsskulder", balanceSek: -174900 },
  { account: "2610", name: "Utgående moms 25 %", balanceSek: -61200 },
];
