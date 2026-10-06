// pm2 en el VPS: /root/siffra/ecosystem.config.cjs  →  pm2 start ecosystem.config.cjs && pm2 save
module.exports = {
  apps: [{
    name: "siffra",
    script: "./target/release/siffra-rs",
    cwd: "/root/siffra/siffra-rs",
    interpreter: "none",
    env: {
      PORT: "3011",
      // Cuentas, sesiones y registro de actividad (SQLite). Hacer copia con `siffra-rs backup`.
      SIFFRA_DB: "/root/siffra/data/siffra.db",
      // Detrás de HTTPS: las cookies de sesión llevan la marca Secure.
      SIFFRA_SECURE_COOKIES: "1",
      // Búsqueda por nombre: índice del archivo oficial de Bolagsverket. El servidor lo descarga y lo renueva
      // cada semana (SIFFRA_REGISTRY_REFRESH=1); la importación tarda ≈30 s y usa ≈100 MB extra de memoria.
      SIFFRA_REGISTRY: "/root/siffra/data/registry.db",
      SIFFRA_REGISTRY_REFRESH: "1",
      // Informes anuales: el ZIP original de cada informe (iXBRL) se guarda aquí; sus datos, en la base de cuentas.
      // Crecen con las empresas consultadas (≈50–200 KB por informe). Incluir la carpeta en las copias de seguridad.
      SIFFRA_REPORTS: "/root/siffra/data/reports",
    },
    max_memory_restart: "700M",
    autorestart: true,
    time: true,
  }],
};
