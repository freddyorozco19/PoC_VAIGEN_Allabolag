"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";

interface NavItem {
  href: string;
  label: string;
  badge?: string;
}

interface NavGroup {
  heading: string;
  items: NavItem[];
}

const NAV: NavGroup[] = [
  {
    heading: "Företag",
    items: [
      { href: "/sok", label: "Sök företag" },
      { href: "/bevakning", label: "Bevakning", badge: "3" },
    ],
  },
  {
    heading: "Min verksamhet",
    items: [
      { href: "/likviditet", label: "Likviditetsprognos" },
      { href: "/sie", label: "Importera SIE" },
      { href: "/fakturor", label: "Fakturor" },
    ],
  },
];

export function Sidebar() {
  const pathname = usePathname();

  return (
    <aside className="flex flex-col gap-[2px] border-r border-line bg-surface2 p-[14px_10px] md:min-h-screen md:w-[220px]">
      <Link href="/sok" className="px-[10px] pb-[12px] pt-[4px] font-display text-[1.3rem] font-bold text-ink">
        Sif<span className="text-accent">f</span>ra
      </Link>
      {NAV.map((group) => (
        <div key={group.heading}>
          <div className="px-[10px] pb-[4px] pt-[14px] font-mono text-[0.68rem] uppercase tracking-[0.09em] text-ink2">
            {group.heading}
          </div>
          {group.items.map((item) => {
            const active = pathname === item.href || pathname.startsWith(item.href + "/");
            return (
              <Link
                key={item.href}
                href={item.href}
                aria-current={active}
                className={`flex items-center justify-between gap-[6px] rounded-[6px] px-[10px] py-[8px] text-[15px] hover:bg-surface ${
                  active ? "bg-surface font-semibold text-accent" : "text-ink"
                }`}
              >
                <span>{item.label}</span>
                {item.badge ? (
                  <span className="rounded-full bg-warn-soft px-[9px] py-[2px] text-[0.75rem] font-semibold text-warn">
                    {item.badge}
                  </span>
                ) : null}
              </Link>
            );
          })}
        </div>
      ))}
    </aside>
  );
}
