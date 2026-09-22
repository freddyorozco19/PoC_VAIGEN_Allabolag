import Link from "next/link";

export default function CompanyNotFound() {
  return (
    <div>
      <h2 className="mb-2 text-[1.7rem]">Företaget hittades inte</h2>
      <p className="mb-3 max-w-[60ch] text-ink2">
        I det här scaffoldet finns bara de tre EJEMPLO-företagen. Sök på namn, organisationsnummer
        eller ort för att hitta dem.
      </p>
      <Link href="/sok" className="text-accent hover:underline">
        ← Tillbaka till sökningen
      </Link>
    </div>
  );
}
