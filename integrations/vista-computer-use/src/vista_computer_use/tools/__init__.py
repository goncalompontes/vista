"""Tool registry assembly."""

from __future__ import annotations

from ..config import Config
from ..host import HostServiceManager
from ..lifecycle import VmManager
from ..qmp import QmpClient
from ..runner import Runner, SubprocessRunner
from ..vphone import ScreenCache, VphoneClient
from . import apps, device, network, vm
from .common import Registry, Services


def build_services(
    config: Config,
    runner: Runner | None = None,
    *,
    cid: int | None = None,
) -> Services:
    runner = runner or SubprocessRunner()
    vm = VmManager(config, runner)
    effective_cid = cid if cid is not None else config.vphone_cid
    if effective_cid is None:
        try:
            effective_cid = vm.resolve(None).cid
        except Exception:  # noqa: BLE001 - status remains usable without a VM
            effective_cid = 3
    vphone = VphoneClient(config, runner, effective_cid)
    qmp = QmpClient(config, runner)
    host = HostServiceManager(config, runner)
    screen = ScreenCache(vphone)
    services = Services(
        config=config,
        vphone=vphone,
        qmp=qmp,
        host=host,
        screen=screen,
        vm=vm,
        vphone_factory=lambda new_cid: VphoneClient(config, runner, new_cid),
    )
    return services


def build_registry(services: Services) -> Registry:
    registry = Registry()
    vm.register(registry, services)
    device.register(registry, services)
    apps.register(registry, services)
    network.register(registry, services)
    from . import vista

    vista.register(registry, services)
    return registry
