from .changes import ChangesController
from .conflicts import ConflictsController
from .history import HistoryController
from .refs import RefsController
from .remote import RemoteController
from .repos import ReposController
from .stash import StashController
from .tools import ToolsController
from .workflow import WorkflowController

controllers = [
    ReposController,
    ChangesController,
    HistoryController,
    RefsController,
    RemoteController,
    StashController,
    WorkflowController,
    ConflictsController,
    ToolsController,
]
