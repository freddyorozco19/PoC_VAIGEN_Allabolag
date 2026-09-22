/**
 * Formas de respuesta esperadas del API gratuito de Bolagsverket
 * ("värdefulla datamängder"). Son una aproximación basada en la
 * documentación pública, sin verificar todavía contra el API real
 * (no hay credenciales — ver ../README.md).
 */

export interface BolagsverketOrganisation {
  organisationsnummer: string;
  namn: string;
  organisationsform: string;
  sni: { kod: string; beskrivning: string }[];
  postadress: {
    gatuadress?: string;
    postnummer?: string;
    postort?: string;
  };
  registreringsdatum: string;
  avregistreringsdatum?: string;
}

export interface BolagsverketAnnualReportRef {
  organisationsnummer: string;
  rakenskapsarSlut: string;
  dokumentId: string;
  /** Referencia al fichero iXBRL original; el contenido se descarga aparte. */
  ixbrlUrl: string;
}
