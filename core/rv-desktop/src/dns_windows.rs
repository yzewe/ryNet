use std::io;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::*;

pub struct FilterSession(pub usize);
impl Drop for FilterSession {
    fn drop(&mut self) {
        unsafe {
            FwpmEngineClose0(self.0 as HANDLE);
        }
    }
}

pub fn install(exe: &[u16], mut luid: u64) -> io::Result<FilterSession> {
    unsafe {
        let mut session = FWPM_SESSION0::default();
        session.flags = FWPM_SESSION_FLAG_DYNAMIC;
        let mut handle: HANDLE = std::ptr::null_mut();
        let check = |status: u32| if status == 0 { Ok(()) } else { Err(io::Error::from_raw_os_error(status as i32)) };
        check(FwpmEngineOpen0(std::ptr::null(), 10, std::ptr::null(), &session, &mut handle))?;
        let held = FilterSession(handle as usize);
        let mut app = std::ptr::null_mut();
        check(FwpmGetAppIdFromFileName0(exe.as_ptr(), &mut app))?;
        let result = (|| {
            check(FwpmTransactionBegin0(handle, 0))?;
            let mut conditions = [FWPM_FILTER_CONDITION0::default(); 4];
            conditions[0].fieldKey = FWPM_CONDITION_IP_REMOTE_PORT;
            conditions[0].matchType = FWP_MATCH_EQUAL;
            conditions[0].conditionValue.r#type = FWP_UINT16;
            conditions[0].conditionValue.Anonymous.uint16 = 53;
            conditions[1].fieldKey = FWPM_CONDITION_IP_NEXTHOP_INTERFACE;
            conditions[1].matchType = FWP_MATCH_NOT_EQUAL;
            conditions[1].conditionValue.r#type = FWP_UINT64;
            conditions[1].conditionValue.Anonymous.uint64 = &mut luid;
            conditions[2].fieldKey = FWPM_CONDITION_ALE_APP_ID;
            conditions[2].matchType = FWP_MATCH_NOT_EQUAL;
            conditions[2].conditionValue.r#type = FWP_BYTE_BLOB_TYPE;
            conditions[2].conditionValue.Anonymous.byteBlob = app;
            conditions[3].fieldKey = FWPM_CONDITION_FLAGS;
            conditions[3].matchType = FWP_MATCH_FLAGS_NONE_SET;
            conditions[3].conditionValue.r#type = FWP_UINT32;
            conditions[3].conditionValue.Anonymous.uint32 = FWP_CONDITION_FLAG_IS_LOOPBACK;
            let mut name: Vec<u16> = "ryNet DNS guard".encode_utf16().chain(Some(0)).collect();
            let mut weight = u64::MAX;
            for layer in [FWPM_LAYER_ALE_AUTH_CONNECT_V4, FWPM_LAYER_ALE_AUTH_CONNECT_V6] {
                let mut filter = FWPM_FILTER0::default();
                filter.displayData.name = name.as_mut_ptr();
                filter.layerKey = layer;
                filter.subLayerKey = FWPM_SUBLAYER_UNIVERSAL;
                filter.action.r#type = FWP_ACTION_BLOCK;
                filter.weight.r#type = FWP_UINT64;
                filter.weight.Anonymous.uint64 = &mut weight;
                filter.numFilterConditions = conditions.len() as u32;
                filter.filterCondition = conditions.as_mut_ptr();
                check(FwpmFilterAdd0(handle, &filter, std::ptr::null_mut(), std::ptr::null_mut()))?;
            }
            check(FwpmTransactionCommit0(handle))
        })();
        FwpmFreeMemory0((&mut app as *mut *mut FWP_BYTE_BLOB).cast());
        result?;
        Ok(held)
    }
}
