/**
 * Modelo de dominio de Siffra.
 *
 * Estos tipos están pensados para servir tanto a los datos de EJEMPLO
 * (src/lib/mock/companies.ts) como a los datos reales que más adelante
 * vendrán de Bolagsverket y SCB (src/lib/data-sources/*). Los nombres de
 * campo siguen la terminología sueca que usan esas fuentes para que el
 * mapeo futuro sea directo.
 */

export type RiskLevel = "good" | "warn" | "bad";

export type AlertSeverity = "good" | "warn" | "bad";

export interface CompanyAlert {
  severity: AlertSeverity;
  text: string;
}

export interface CompanyPerson {
  role: string;
  name: string;
}

/** Cinco años de una misma magnitud, en miles de kronor (tkr), del más antiguo al más reciente. */
export type FiveYearSeries = readonly [number, number, number, number, number];

export interface FinancialHistory {
  /** Omsättning: ingresos netos. */
  revenue: FiveYearSeries;
  /** Resultat efter finansiella poster. */
  result: FiveYearSeries;
  /** Eget kapital. */
  equity: FiveYearSeries;
  /** Summa tillgångar. */
  totalAssets: FiveYearSeries;
}

/** value = la propia empresa, median = mediana del sector (SNI) en el mismo periodo. */
export interface BenchmarkPair {
  value: number;
  median: number;
}

export interface CompanyBenchmarks {
  /** Vinstmarginal, en %. */
  margin: BenchmarkPair;
  /** Soliditet, en %. */
  solidity: BenchmarkPair;
  /** Kassalikviditet, en %. */
  liquidity: BenchmarkPair;
}

export interface Company {
  id: string;
  name: string;
  /** Organisationsnummer sueco, formato NNNNNN-NNNN. */
  orgNumber: string;
  /** Bolagsform, p.ej. "Aktiebolag". */
  legalForm: string;
  /** Código y descripción SNI (actividad económica). */
  sni: string;
  city: string;
  /** Tramo de empleados según SCB, p.ej. "10–19". */
  employeeRange: string;
  status: string;
  riskLevel: RiskLevel;
  firmateckning: string;
  people: CompanyPerson[];
  alerts: CompanyAlert[];
  financials: FinancialHistory;
  benchmarks: CompanyBenchmarks;
}

/** Los cinco años calendario que cubren las series de FinancialHistory, del más antiguo al más reciente. */
export const FINANCIAL_YEARS = ["2020", "2021", "2022", "2023", "2024"] as const;

export interface WatchAlert {
  severity: AlertSeverity;
  companyName: string;
  text: string;
  when: string;
}

export interface CashWeek {
  week: number;
  inflow: number;
  outflow: number;
}

export interface Invoice {
  number: string;
  customer: string;
  amountSek: number;
  dueDate: string;
  status: "Förfallen" | "Obetald" | "Betald";
  severity: AlertSeverity;
}

export interface SieAccountPreview {
  account: string;
  name: string;
  balanceSek: number;
}
