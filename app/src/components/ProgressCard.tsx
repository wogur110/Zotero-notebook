import { IconLoader } from "./icons";

interface Props {
  title: string;
  subtitle: string;
  /** 0–100 */
  pct: number;
}

/** Centered progress panel shared by the classify, audit, and apply flows. */
export default function ProgressCard({ title, subtitle, pct }: Props) {
  return (
    <div className="flex h-full items-center justify-center p-6">
      <div className="card w-[440px] max-w-[90vw] p-6 text-center">
        <div className="mx-auto mb-3 flex h-11 w-11 items-center justify-center rounded-full bg-accent-soft text-accent">
          <IconLoader size={20} />
        </div>
        <p className="font-medium">{title}</p>
        <p className="mt-1 h-5 truncate text-sm text-muted">{subtitle}</p>
        <div className="mt-4 h-1 overflow-hidden rounded-full bg-inset">
          <div
            className="h-full bg-accent transition-all duration-300"
            style={{ width: `${pct}%` }}
          />
        </div>
      </div>
    </div>
  );
}
