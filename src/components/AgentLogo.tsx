import { Bot } from "lucide-react";
import clsx from "clsx";
import autoclawIcon from "../assets/agents/autoclaw.png";
import codebuddyIcon from "../assets/agents/codebuddy.png";
import codexIcon from "../assets/agents/codex.png";
import dshIcon from "../assets/agents/dsh.png";
import dumateIcon from "../assets/agents/dumate.png";
import easyclawIcon from "../assets/agents/easyclaw.png";
import hermesIcon from "../assets/agents/hermes.png";
import imaIcon from "../assets/agents/ima.png";
import kimiworkIcon from "../assets/agents/kimiwork.png";
import opencodeIcon from "../assets/agents/opencode.png";
import qclawIcon from "../assets/agents/qclaw.png";
import workbuddyIcon from "../assets/agents/workbuddy.png";
import aionclawIcon from "../assets/agents/aionclaw.png";
import traecodeIcon from "../assets/agents/traecode.png";
import traeworkIcon from "../assets/agents/traework.png";
import zcodeIcon from "../assets/agents/zcode.png";

const agentLogos: Partial<Record<string, string>> = {
  workbuddy: workbuddyIcon,
  codebuddy: codebuddyIcon,
  qclaw: qclawIcon,
  autoclaw: autoclawIcon,
  codex: codexIcon,
  dsh: dshIcon,
  dumate: dumateIcon,
  hermes: hermesIcon,
  opencode: opencodeIcon,
  kimiwork: kimiworkIcon,
  aionclaw: aionclawIcon,
  zcode: zcodeIcon,
  traework: traeworkIcon,
  traecode: traecodeIcon,
  easyclaw: easyclawIcon,
  ima: imaIcon,
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
      ) : (
        <Bot strokeWidth={1.8} />
      )}
    </span>
  );
}
