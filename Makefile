docker:
	@docker build -t dht_kad_rs .

run-testbed:
	@cd testbed && docker-compose up --scale=dht=5	
