/**
 * STUB — sin credenciales todavía.
 *
 * Cliente para el API gratuito de Bolagsverket ("värdefulla datamängder").
 * Requiere completar la kundanmälan y obtener client_id/client_secret de
 * OAuth2 (ver ../README.md) antes de implementar nada aquí. Límite
 * documentado: 60 peticiones/minuto.
 *
 * Las firmas de las funciones son estables a propósito: el resto de la app
 * (búsqueda, ficha de empresa) puede importarlas ya, y el día que haya
 * credenciales solo hace falta rellenar el cuerpo.
 */
import "server-only";
import type { BolagsverketAnnualReportRef, BolagsverketOrganisation } from "./types";

export interface BolagsverketConfig {
  clientId: string;
  clientSecret: string;
  baseUrl: string;
}

export function loadBolagsverketConfig(): BolagsverketConfig | null {
  const clientId = process.env.BOLAGSVERKET_CLIENT_ID;
  const clientSecret = process.env.BOLAGSVERKET_CLIENT_SECRET;
  const baseUrl = process.env.BOLAGSVERKET_BASE_URL;
  if (!clientId || !clientSecret || !baseUrl) return null;
  return { clientId, clientSecret, baseUrl };
}

function notImplemented(operation: string): never {
  throw new Error(
    `Bolagsverket: "${operation}" no está implementado todavía. ` +
      "Falta completar la kundanmälan y configurar BOLAGSVERKET_CLIENT_ID / " +
      "BOLAGSVERKET_CLIENT_SECRET / BOLAGSVERKET_BASE_URL (ver .env.example)."
  );
}

export async function getOrganisationByNumber(
  _organisationsnummer: string
): Promise<BolagsverketOrganisation> {
  return notImplemented("getOrganisationByNumber");
}

export async function searchOrganisations(_query: string): Promise<BolagsverketOrganisation[]> {
  return notImplemented("searchOrganisations");
}

export async function getLatestAnnualReportRef(
  _organisationsnummer: string
): Promise<BolagsverketAnnualReportRef> {
  return notImplemented("getLatestAnnualReportRef");
}
