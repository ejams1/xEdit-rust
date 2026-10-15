unit P_t_25;

function Initialize: integer;
begin
  AddMessage('init');
end;

function Process(e: IInterface): integer;
begin
  AddMessage('p1');
end;

function Process(e: IInterface): integer;
begin
end;

end.
