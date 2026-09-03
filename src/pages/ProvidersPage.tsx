import {
  Pencil,
  KeyRound,
  Plus,
  ShieldAlert,
  Trash2,
} from "lucide-react";
import type { ProviderSummary } from "../types";
import { useLanguage } from "../i18n";
import { providerProtocolLabel } from "../lib/format";
import { PageHeader } from "../components/PageHeader";
import { ProviderLogo } from "../components/ProviderLogo";
import { modelRequiresVerification } from "../lib/modelCapabilities";

interface ProvidersPageProps {
  providers: ProviderSummary[];
  testingId?: string;
  onCreate: () => void;
  onEdit: (provider: ProviderSummary) => void;
  onDelete: (provider: ProviderSummary) => void;
  onTest: (providerId: string, modelId?: string) => void;
}

export function ProvidersPage({
  providers,
  onCreate,
  onEdit,
  onDelete,
}: ProvidersPageProps) {
  const { text } = useLanguage();
  return (
    <>
      <PageHeader
        eyebrow="MODEL SOURCES / 03"
        title={text("模型供应商与大模型", "Model providers & LLMs")}
        description={text(
          "集中维护模型供应商、API Key 和大模型目录；文本模型验证后即可切换，生图、语音和视频模型无需验证。",
          "Manage model providers, API keys, and model catalogs in one place. Text models require verification; image-generation, audio, and video models do not.",
        )}
        actions={
          <button
            className="button button--primary provider-create-action"
            onClick={onCreate}
          >
            <Plus size={21} />
            {text("新建模型供应商", "New model provider")}
          </button>
        }
      />

      <section className="provider-grid">
        {providers.map((provider) => {
          const verifiableModels = provider.models.filter(
            modelRequiresVerification,
          );
          const verifiedCount = verifiableModels.filter(
            (model) => model.verificationStatus === "verified",
          ).length;
          return (
            <article
              className={`provider-card ${
                provider.isRecommended ? "provider-card--recommended" : ""
              }`}
              key={provider.id}
            >
              <div className="provider-card__top">
                <ProviderLogo provider={provider} size="card" />
                <div className="provider-card__identity">
                  <div>
                    <h2>{provider.name}</h2>
                  </div>
                  <p>{providerProtocolLabel(provider)}</p>
                </div>
                <div className="provider-card__actions">
                  <button
                    className="icon-button"
                    aria-label={text(
                      `编辑 ${provider.name}`,
                      `Edit ${provider.name}`,
                    )}
                    onClick={() => onEdit(provider)}
                  >
                    <Pencil size={16} />
                  </button>
                  <button
                    className="icon-button icon-button--danger"
                    aria-label={text(
                      `删除 ${provider.name}`,
                      `Delete ${provider.name}`,
                    )}
                    onClick={() => onDelete(provider)}
                  >
                    <Trash2 size={16} />
                  </button>
                </div>
              </div>

              <dl className="provider-details">
                <div>
                  <dt>Endpoint</dt>
                  <dd title={provider.baseUrl}>{provider.baseUrl}</dd>
                </div>
                <div>
                  <dt>API Key</dt>
                  <dd>
                    <KeyRound size={14} />
                    {provider.hasApiKey
                      ? (provider.maskedApiKey ??
                        text("已安全保存", "Stored securely"))
                      : text("尚未填写", "Not provided")}
                  </dd>
                </div>
                {verifiableModels.length > 0 && (
                  <div>
                    <dt>{text("已验证文本模型", "Verified text models")}</dt>
                    <dd>
                      {verifiedCount}/{verifiableModels.length}
                    </dd>
                  </div>
                )}
              </dl>

              <div className="provider-models">
                <div className="provider-models__header">
                  <span>{text("模型目录", "Model catalog")}</span>
                  <strong>{provider.models.length}</strong>
                </div>
                {provider.models.length > 0 ? (
                  <div
                    className="provider-models__list"
                    aria-label={text(
                      `${provider.name} 模型列表`,
                      `${provider.name} model list`,
                    )}
                  >
                    {provider.models.map((model) => {
                      return (
                        <span
                          className="provider-model-chip"
                          key={model.id}
                          title={`${model.displayName} · ${model.modelId}`}
                        >
                          {model.displayName}
                        </span>
                      );
                    })}
                  </div>
                ) : (
                  <span className="provider-models__empty">
                    {text("尚未配置模型", "No models configured")}
                  </span>
                )}
              </div>


            </article>
          );
        })}

        <button className="provider-card provider-card--new" onClick={onCreate}>
          <span className="provider-card--new__icon">
            <Plus size={24} />
          </span>
          <strong>{text("添加新的模型供应商", "Add a model provider")}</strong>
          <small>
            {text(
              "手工填写三协议兼容接口",
              "Configure a compatible endpoint manually",
            )}
          </small>
        </button>
      </section>

      <section className="inline-banner">
        <ShieldAlert size={20} />
        <div>
          <strong>
            {text("真实密钥与界面分离", "Real keys stay out of the interface")}
          </strong>
          <span>
            {text(
              "Windows 使用 Credential Manager，macOS 使用 Keychain。列表只显示引用和脱敏尾部。",
              "Windows uses Credential Manager and macOS uses Keychain. Lists show only references and masked suffixes.",
            )}
          </span>
        </div>
      </section>
    </>
  );
}
