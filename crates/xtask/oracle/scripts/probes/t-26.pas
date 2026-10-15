unit P_t_26;

function Initialize: integer;
begin
  AddMessage('init');
end;

function Process(e: IInterface): integer;
begin
end;

function Process(e: IInterface): integer;
begin
  AddMessage('p2');
end;

end.
