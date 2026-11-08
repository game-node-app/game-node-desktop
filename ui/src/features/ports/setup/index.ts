/**
 * Public surface of the PC ports guided setup.
 *
 * `PortSetupHost` is mounted once inside `PortsView`. It owns the setup modal, the job-result
 * notifications, the missing-executable follow-up and the failure details.
 */
export { openPortSetup } from './setupStore';
export { PortSetupHost } from './PortSetupHost';
export { PortFailureDetails } from './PortFailureDetails';
