/**
 * STUB — acceso todavía no solicitado.
 *
 * Cliente para el Företagsregistret de SCB (gratuito desde junio de 2025).
 * El acceso se pide por email a scbforetag@scb.se (no automatizado por
 * kundanmälan como Bolagsverket). Límites documentados: máx. 2000 filas por
 * consulta, 10 consultas/10s, actualización nocturna.
 *
 * SCB anunció una nueva versión del API para septiembre de 2026 — revisar
 * si hay cambios de contrato antes de implementar esto.
 */
import "server-only";
import type { ScbCompanyRecord } from "./types";

export interface ScbConfig {
  apiKey: string;
  baseUrl: string;
}

export function loadScbConfig(): ScbConfig | null {
  const apiKey = process.env.SCB_API_KEY;
  const baseUrl = process.env.SCB_BASE_URL;
  if (!apiKey || !baseUrl) return null;
  return { apiKey, baseUrl };
}

function notImplemented(operation: string): never {
  throw new Error(
    `SCB: "${operation}" no está implementado todavía. ` +
      "Falta solicitar acceso a scbforetag@scb.se y configurar " +
      "SCB_API_KEY / SCB_BASE_URL (ver .env.example)."
  );
}

export async function getCompanyByOrgNumber(
  _organisationsnummer: string
): Promise<ScbCompanyRecord> {
  return notImplemented("getCompanyByOrgNumber");
}

export async function searchCompaniesBySni(_sniKod: string): Promise<ScbCompanyRecord[]> {
  return notImplemented("searchCompaniesBySni");
}
