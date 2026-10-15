unit P_t_23;

function Initialize: integer;
begin
  AddMessage('init');
end;

function Process(e: IInterface): integer;
begin
  Result := 0;;
end;

function Finalize: integer;
begin
  AddMessage('fin');
end;

end.
