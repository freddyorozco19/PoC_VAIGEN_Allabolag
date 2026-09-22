/**
 * Forma de respuesta esperada del Företagsregistret de SCB.
 * Aproximación basada en la documentación pública; sin verificar todavía
 * contra el API real (acceso pendiente de solicitar por email — ver
 * ../README.md).
 */
export interface ScbCompanyRecord {
  organisationsnummer: string;
  namn: string;
  sniKod: string;
  sniBeskrivning: string;
  kommun: string;
  lan: string;
  /** Tramo de empleados, p.ej. "10-19". */
  anstalldaIntervall: string;
}
