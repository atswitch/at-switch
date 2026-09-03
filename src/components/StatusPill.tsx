import clsx from "clsx";

interface StatusPillProps {
  tone: "good" | "warn" | "bad" | "neutral" | "active";
  children: React.ReactNode;
  pulse?: boolean;
}

export function StatusPill({ tone, children, pulse = false }: StatusPillProps) {
  return (
    <span className={clsx("status-pill", `status-pill--${tone}`)}>
      <span className={clsx("status-pill__dot", pulse && "is-pulsing")} />
      {children}
    </span>
  );
}
