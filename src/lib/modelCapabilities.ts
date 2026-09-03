import type { ModelSummary } from "../types";

type ModelCapabilityIdentity = Pick<
  ModelSummary,
  "outputModality" | "verificationStatus"
>;

export function modelRequiresVerification(
  model: Pick<ModelCapabilityIdentity, "outputModality">,
) {
  return model.outputModality === "text";
}

export function modelIsReady(model: ModelCapabilityIdentity) {
  return (
    !modelRequiresVerification(model) ||
    model.verificationStatus === "verified"
  );
}
