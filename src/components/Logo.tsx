import logoMark from "../assets/logo-mark.png";

interface LogoProps {
  size?: number;
  withWordmark?: boolean;
  className?: string;
}

export function Logo({ size = 32, withWordmark = false, className }: LogoProps) {
  return (
    <div
      className={className}
      style={{ display: "flex", alignItems: "center", gap: 10 }}
    >
      <img src={logoMark} width={size} height={size} alt="" aria-hidden="true" />
      {withWordmark && (
        <span
          style={{
            fontSize: size * 0.55,
            fontWeight: 700,
            letterSpacing: "-0.01em",
          }}
        >
          Wervik
        </span>
      )}
    </div>
  );
}
