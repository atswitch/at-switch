import logoUrl from "../../src-tauri/icons/icon.png";

interface BrandLogoProps {
  variant: "toolbar" | "boot";
}

export function BrandLogo({ variant }: BrandLogoProps) {
  return (
    <img
      src={logoUrl}
      className={`brand-logo brand-logo--${variant}`}
      alt=""
      aria-hidden="true"
      draggable={false}
    />
  );
}
