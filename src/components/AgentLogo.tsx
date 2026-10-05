import { BookOpen, Bot } from "lucide-react";
import clsx from "clsx";
import autoclawIcon from "../assets/agents/autoclaw.png";
import codebuddyIcon from "../assets/agents/codebuddy.png";
import codexIcon from "../assets/agents/codex.png";
import dumateIcon from "../assets/agents/dumate.png";
import qclawIcon from "../assets/agents/qclaw.png";
import workbuddyIcon from "../assets/agents/workbuddy.png";

const agentLogos: Partial<Record<string, string>> = {
  workbuddy: workbuddyIcon,
  codebuddy: codebuddyIcon,
  qclaw: qclawIcon,
  autoclaw: autoclawIcon,
  codex: codexIcon,
  dumate: dumateIcon,
};

interface AgentLogoProps {
  agentId: string;
  className?: string;
}

export function AgentLogo({ agentId, className }: AgentLogoProps) {
  const logo = agentLogos[agentId];

  return (
    <span
      className={clsx("agent-logo", className)}
      data-agent-logo={agentId}
      aria-hidden="true"
    >
      {logo ? (
        <img src={logo} alt="" />
      ) : agentId === "ima" ? (
        <BookOpen strokeWidth={1.8} />
      ) : (
        <Bot strokeWidth={1.8} />
      )}
    </span>
  );
}
