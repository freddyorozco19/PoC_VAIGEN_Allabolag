# Siffra (nombre de trabajo)

Plataforma de inteligencia financiera de empresas para el mercado sueco —
una alternativa a Allabolag.se que además analiza la propia empresa del
usuario (importación de contabilidad SIE, previsión de caja, comparación
con el sector).

El plan de producto completo, el mockup interactivo original y las
decisiones ya tomadas viven en el Claude Artifact **"Siffra MVP"**:
https://claude.ai/artifact/8ktdHbhcQzGda4sHcXbJLw (6 pestañas: Visión,
Mockup interactivo, Funcionalidades, Plan de trabajo, Arquitectura y datos,
Riesgos y costes). Este repositorio es el scaffold de código que arranca
ese plan; ante cualquier duda de producto, esa es la fuente de verdad.

## Estado real del proyecto (lo importante)

**Esto es un scaffold navegable, no un producto conectado a datos reales.**

| Parte | Estado |
|---|---|
| Estructura Next.js (App Router, TypeScript estricto, Tailwind) | ✅ Real |
| Páginas: Sök företag, ficha de empresa, Bevakning, Likviditetsprognos, Importera SIE, Fakturor | ✅ Real (código funcional) |
| Datos que se ven en pantalla (3 empresas, alertas, facturas, previsión de caja) | ⚠️ **Ficticios**, marcados con la etiqueta `EJEMPLO` en la UI |
| Ingesta de Bolagsverket / SCB | ❌ Solo stubs tipados en `src/lib/data-sources/`, sin llamadas de red. No hay credenciales todavía |
| Lector de iXBRL, parser SIE, previsión de caja real, resumen con IA real | ❌ No implementado |
| Login | ❌ No implementado |
| i18n sueco/inglés | ⚠️ La interfaz está en sueco (como la verá el cliente final); el soporte de idioma inglés desde el día 1 (decisión de producto) queda pendiente de cablear con una librería de i18n |

Todo dato ficticio en la aplicación lleva la etiqueta `EJEMPLO` junto al
dato o en una nota al pie, tal como en el mockup original.

## Empezar en local

```bash
npm install
npm run dev     # http://localhost:3000
```

Otros comandos:

```bash
npm run build      # build de producción
npm run lint       # ESLint (next/core-web-vitals + next/typescript)
npm run typecheck  # tsc --noEmit, con TypeScript estricto
```

## Estructura

```
src/
  app/
    sok/                  Sök företag — búsqueda de empresas
    foretag/[org]/         Ficha de empresa (KPIs, Bokslut, Personer, Sammanfattning)
    bevakning/             Alertas de empresas seguidas
    likviditet/            Previsión de caja a 13 semanas
    sie/                   Importación de contabilidad SIE (mock de pantalla)
    fakturor/              Facturas de cliente (mock de pantalla)
  components/              Componentes de UI compartidos (sidebar, pills, gráficos SVG)
  lib/
    types.ts               Modelo de dominio (Company, FinancialHistory, ...)
    mock/companies.ts       Datos de EJEMPLO: Nordlys Logistik AB, Fjällbruk Bygg & Design AB,
                            Kvarn & Krydda Livs AB — copiados del mockup validado
    ai-summary.ts           Generador de resumen en sueco — STUB, no llama a ningún modelo
    data-sources/
      bolagsverket/          Cliente STUB del API gratuito de Bolagsverket
      scb/                   Cliente STUB del Företagsregistret de SCB
      README.md              Por qué no hay llamadas reales todavía y qué hacer cuando las haya
```

Los tipos en `src/lib/types.ts` y las interfaces en `src/lib/data-sources/`
están pensados para que sustituir los datos de ejemplo por datos reales sea
un cambio de implementación, no de diseño.

## Diseño

Los tokens de color (tema claro/oscuro), tipografías (Bricolage Grotesque,
IBM Plex Sans, IBM Plex Mono) y componentes visuales (KPIs, pills de riesgo,
gráficos de barras SVG) replican exactamente el mockup interactivo validado
en el Artifact, para no reabrir decisiones de diseño ya tomadas.

## Próximos pasos reales (no hechos en este scaffold)

Trámites (bloquean todo lo demás):
1. Enviar la *kundanmälan* a Bolagsverket para obtener `client_id`/`client_secret`
   de OAuth2 del API gratuito "värdefulla datamängder".
2. Escribir a `scbforetag@scb.se` para pedir acceso al Företagsregistret
   (gratuito desde junio de 2025). **Revisar si ya hay cambios de la nueva
   versión de API que SCB anunció para septiembre de 2026.**
3. Decidir la base legal RGPD (qué datos de personas se guardan y por cuánto
   tiempo) antes de tocar el módulo de Personer.

Ingesta y backend (Fase 1 del plan, semanas 3–6):
4. Implementar de verdad `src/lib/data-sources/bolagsverket/client.ts` y
   `scb/client.ts` contra las APIs reales.
5. Levantar PostgreSQL y los workers de ingesta (carga inicial + actualización
   periódica) descritos en la pestaña "Arquitectura y datos" del plan.
6. Construir el lector de iXBRL (empezar con K2 y ~20 etiquetas de cifras
   principales) — es la pieza técnica más incierta del proyecto.
7. Sustituir `src/lib/mock/companies.ts` por datos reales en las páginas de
   búsqueda y ficha de empresa; mantener el fichero como fixture de demo/tests.
8. Implementar login por email.

Después (Fases 2 y 3):
9. Ratios/percentiles reales por SNI, alertas por email, señales de riesgo
   basadas en reglas, resumen con IA real (sustituir `ai-summary.ts`),
   export PDF/Excel.
10. Parser de ficheros SIE 4 y mapeo al plan de cuentas BAS; previsión de
    caja real a 13 semanas.
11. Pruebas con las 5 pymes/asesores piloto.

Nota de seguridad de dependencias: `npm audit` señala una vulnerabilidad
alta en una copia de `postcss` empaquetada internamente por Next.js 15
(no la que usa Tailwind en este proyecto). Arreglarla exige saltar a
Next.js 16 (breaking change). Se deja pendiente a propósito para no
desestabilizar el scaffold recién creado; revisar antes de ir a producción.
