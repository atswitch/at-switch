interface PanelEmptyStateProps {
  icon: React.ReactNode;
  title: string;
  description: string;
}

/** 面板内统一的空状态：居中、带图标，避免裸段落贴在面板里显得零散。 */
export function PanelEmptyState({
  icon,
  title,
  description,
}: PanelEmptyStateProps) {
  return (
    <div className="panel-empty" role="status">
      <span className="panel-empty__icon" aria-hidden="true">
        {icon}
      </span>
      <strong>{title}</strong>
      <span>{description}</span>
    </div>
  );
}
