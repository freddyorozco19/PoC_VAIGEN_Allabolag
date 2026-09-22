export const metadata = {
  title: "Sök företag — Siffra",
};

import { SearchClient } from "./SearchClient";

export default function SokPage() {
  return (
    <div>
      <h2 className="mb-1 text-[1.7rem]">Sök företag</h2>
      <p className="mb-4 max-w-[68ch] text-ink2">
        Interfaz de producto en sueco. Estas tres empresas son un{" "}
        <strong>EJEMPLO</strong> ficticio; en producción esta lista vendrá de
        SCB y Bolagsverket.
      </p>
      <SearchClient />
    </div>
  );
}
