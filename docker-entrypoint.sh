BITCOIN_DATA_DIR=${BITCOIN_DATA_DIR:-/bitcoin/.bitcoin}
DATA_DIR=$BITCOIN_DATA_DIR
if [ "$NETWORK" = "testnet" ]; then
  DATA_DIR+= "/testnet3"
else if [ "$NETWORK" != "mainnet" ]; then
  DATA_DIR+=/"$NETWORK"
fi
fi

COOKIE_FILENAME=.cookie
  

_HTTP_ADDR=${HTTP_ADDR:-127.0.0.1:50010}
_DB_DIR=${DB_DIR:-/bitcoin/.bitcoin/.electrs}
_DAEMON_DIR=${DAEMON_DIR:-/bitcoin/.bitcoin}
_DAEMON_RPC_ADDR=${DAEMON_RPC_ADDR:-127.0.0.1:8332}
_NETWORK=${NETWORK:-bitcoin}
_MONITORING_ADDR=${MONITORING_ADDR:-0.0.0.0:4224}
COOKIE_REALPATH=${COOKIE_FILE:-$DATA_DIR/$COOKIE_FILENAME}
_BITCOINCONF_PATH=${BITCOINCONF_PATH:-$BITCOIN_DATA_DIR/bitcoin.conf}

if [ -f $COOKIE_REALPATH ]; then
  RPCPASS=$(cat $COOKIE_REALPATH | cut -f 2 -d :);
else
  RPCUSER=$(awk -F = '$1 ~ /rpcuser/ { print $0 }' $_BITCOINCONF_PATH | sed -e 's/rpcuser=//')
  RPCPASS=$(awk -F = '$1 ~ /rpcpassword/ { print $0 }' $_BITCOINCONF_PATH | sed -e 's/rpcpassword=//')
  echo -n "$RPCUSER:$RPCPASS" > $COOKIE_REALPATH
fi

echo "HTTP_ADDR: $_HTTP_ADDR"
echo "DB_DIR: $_DB_DIR"
echo "DAEMON_DIR: $_DAEMON_DIR"
echo "DAEMON_RPC_ADDR: $_DAEMON_RPC_ADDR"
echo "NETWORK: $_NETWORK"
echo "MONITORING_ADDR: $_MONITORING_ADDR"
echo "COOKIE_REALPATH: $COOKIE_REALPATH"
echo "COOKIE: $(cat $COOKIE_REALPATH)"
echo "BITCOINCONF_PATH: $_BITCOINCONF_PATH"



#echo "waiting on $COOKIE_REALPATH"
#echo 1 >> /tmp/run.pid
#until [ -f $COOKIE_REALPATH ]
#do
#  sleep 5
#done
/build/electrs/target/debug/mainnet --log-filters DEBUG --db-dir $_DB_DIR --daemon-dir $_DAEMON_DIR --network $_NETWORK --monitoring-addr $_MONITORING_ADDR --cookie $COOKIE_FILENAME
