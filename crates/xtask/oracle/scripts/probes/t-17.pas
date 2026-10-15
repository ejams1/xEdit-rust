unit P_t_17;

function Initialize: integer;
begin
  AddMessage('init');
end;

function Process(e: IInterface): integer;
begin
  Result := 1;
end;

function Finalize: integer;
begin
  AddMessage('fin');
end;

end.
