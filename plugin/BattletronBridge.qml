import QtQuick.Layouts

Item {
    anchors.fill: parent
    Flickable {
        id: deviceView
        anchors.fill: parent
        contentHeight: deviceList.height
        clip: true
    Column {
        id: deviceList
        width: deviceView.width
        spacing: 14
        Pane {
            width: parent.width
            height: 225
            background: Rectangle { color: "#101b13"; radius: 14; border.color: "#315d31" }
            Column {
                spacing: 10
                Text { text: "STARGATELABS · Battletron Bridge"; color: "#a5ff63"; font.pixelSize: 23 }
                Text { text: "Le luci devono essere già associate a LSC Smart Connect."; color: "#cad6c5"; font.pixelSize: 13 }
                Text { text: "Bridge locale: 127.0.0.1:21444"; color: "#9caa98"; font.pixelSize: 12 }
                Text { id: bridgeStatus; text: "Verifica connessione..."; color: "#cad6c5"; font.pixelSize: 12 }
                SButton {
                    width: 180; height: 36
                    label.text: "Aggiorna dispositivi"
                    onClicked: discovery.Update(true)
                }
                SButton {
                    width: 220; height: 36
                    label.text: "Riconnetti tutti"
                    onClicked: discovery.Reconnect("")
                }
            }
        }
        Repeater {
            model: service.controllers
            delegate: Pane {
                width: parent.width
                height: 124
                property var controllerInfo: model.modelData.obj
                background: Rectangle { color: "#141a16"; radius: 10; border.color: "#344536" }
                Column {
                    spacing: 7
                    Text { text: controllerInfo.name; color: "white"; font.pixelSize: 18 }
                    Text { text: "ID: " + controllerInfo.id + " · 4 zone colore"; color: "#9caa98"; font.pixelSize: 11 }
                    Text { id: onlineStatus; text: "Verifica stato..."; color: "#cad6c5"; font.pixelSize: 12 }
                    SButton {
                        width: 180; height: 30
                        label.text: "Riconnetti"
                        onClicked: discovery.Reconnect(controllerInfo.id)
                    }
                }
                Timer {
                    interval: 1000; running: true; repeat: true
                    onTriggered: onlineStatus.text = discovery.IsOnline(controllerInfo.id) ? "Online" : "Offline · controlla alimentazione e Wi-Fi"
                }
            }
        }
    }
    }
    Timer {
        interval: 1000; running: true; repeat: true
        onTriggered: bridgeStatus.text = discovery.status
    }
}
