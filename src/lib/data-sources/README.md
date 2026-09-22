# Fuentes de datos externas

**Estado: sin implementar.** Nada en esta carpeta hace llamadas de red reales.
Son *stubs* tipados para que el resto del código (búsqueda, ficha de empresa,
ingesta) se pueda escribir ya contra una interfaz estable, y para que
conectar los datos reales más adelante sea sustituir la implementación de
estas funciones, no rediseñar el resto de la app.

## Por qué no hay llamadas reales todavía

- **Bolagsverket** (API gratuito "värdefulla datamängder"): requiere una
  *kundanmälan* (alta de cliente) para obtener `client_id`/`client_secret`
  de OAuth2. Ese trámite no se ha hecho.
- **SCB** (Företagsregistret): el acceso es gratuito desde junio de 2025 pero
  se solicita por correo a `scbforetag@scb.se`; tampoco se ha hecho.

Ver la pestaña "Riesgos y costes" / "Arquitectura y datos" del plan (Claude
Artifact "Siffra MVP") para el detalle de límites de tasa, formatos y costes.

## Qué hacer cuando lleguen las credenciales

1. Copiar `.env.example` a `.env.local` y rellenar las variables.
2. Sustituir el cuerpo de las funciones en `bolagsverket/client.ts` y
   `scb/client.ts` por llamadas HTTP reales, manteniendo la firma de las
   funciones (mismos tipos de entrada/salida) para no romper quien las usa.
3. Quitar el `throw` de "no implementado" y el aviso de EJEMPLO en la UI que
   consuma estos datos.
4. Añadir tests contra las respuestas reales de cada API antes de mapear a
   `src/lib/types.ts`.
