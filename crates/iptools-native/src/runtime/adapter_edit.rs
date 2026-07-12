use iptools_core::{
    AdapterApplyOutcome, AdapterConfigRequest, JobId, RuntimeError, RuntimeErrorCode, RuntimeEvent,
};

use super::{NativeRuntime, RuntimeTaskError};

impl NativeRuntime {
    pub(super) fn spawn_adapter_config(&mut self, job: JobId, request: AdapterConfigRequest) {
        let gate = self.adapter_gate.clone();
        self.spawn(job, move |_, events| async move {
            events
                .send(RuntimeEvent::AdapterConfigStarted { job })
                .await
                .map_err(send_error)?;

            let _permit = gate
                .acquire_owned()
                .await
                .map_err(|error| RuntimeTaskError::Operation(error.to_string()))?;

            let result = tokio::task::spawn_blocking(move || apply(request))
                .await
                .map_err(|error| RuntimeTaskError::Operation(error.to_string()))?;
            let event = match result {
                Ok(outcome) => RuntimeEvent::AdapterConfigFinished { job, outcome },
                Err(error) => RuntimeEvent::AdapterConfigFailed { job, error },
            };
            events.send(event).await.map_err(send_error)?;
            Ok(())
        });
    }
}

fn apply(request: AdapterConfigRequest) -> Result<AdapterApplyOutcome, RuntimeError> {
    if request.guid.trim().is_empty() {
        return Err(RuntimeError::new(
            RuntimeErrorCode::InvalidRequest,
            "adapter identifier is empty",
        ));
    }
    request
        .validate()
        .map_err(|error| RuntimeError::new(RuntimeErrorCode::InvalidRequest, error.to_string()))?;
    let result = if request.use_dhcp {
        crate::utils::ipconfig::apply_dhcp(&request.guid)
    } else {
        crate::utils::ipconfig::apply_static(
            &request.guid,
            &request.ip,
            &request.mask,
            request.gateway.as_deref(),
            &request.dns,
        )
    };
    map_apply_result(result)
}

fn map_apply_result(result: Result<(), String>) -> Result<AdapterApplyOutcome, RuntimeError> {
    match result {
        Ok(()) => Ok(AdapterApplyOutcome::Persistent),
        Err(message) if message == "__IP_RUNTIME_ONLY__" => Ok(AdapterApplyOutcome::RuntimeOnly),
        Err(message) => {
            let lowercase = message.to_lowercase();
            let code = if [
                "access",
                "permission",
                "administrator",
                "权限",
                "管理员",
                "拒绝",
            ]
            .iter()
            .any(|needle| lowercase.contains(needle))
            {
                RuntimeErrorCode::PermissionDenied
            } else {
                RuntimeErrorCode::Network
            };
            Err(RuntimeError::new(code, message))
        }
    }
}

fn send_error(error: tokio::sync::mpsc::error::SendError<RuntimeEvent>) -> RuntimeTaskError {
    RuntimeTaskError::Operation(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_persistent_and_runtime_only_results() {
        assert_eq!(
            map_apply_result(Ok(())).unwrap(),
            AdapterApplyOutcome::Persistent
        );
        assert_eq!(
            map_apply_result(Err("__IP_RUNTIME_ONLY__".into())).unwrap(),
            AdapterApplyOutcome::RuntimeOnly
        );
    }

    #[test]
    fn classifies_privilege_errors_without_touching_the_network() {
        let error = map_apply_result(Err("需要管理员权限".into())).unwrap_err();
        assert_eq!(error.code, RuntimeErrorCode::PermissionDenied);
        let error = map_apply_result(Err("adapter not found".into())).unwrap_err();
        assert_eq!(error.code, RuntimeErrorCode::Network);
    }

    #[test]
    fn invalid_requests_are_rejected_before_system_network_io() {
        let error = apply(AdapterConfigRequest {
            guid: "test-adapter".into(),
            name: "Test".into(),
            use_dhcp: false,
            ip: "not-an-ip".into(),
            mask: "255.255.255.0".into(),
            gateway: None,
            dns: Vec::new(),
        })
        .unwrap_err();
        assert_eq!(error.code, RuntimeErrorCode::InvalidRequest);
    }
}
