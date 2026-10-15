unit P_t_29;

function Initialize: integer;
begin
  AddMessage('init');
end;

function Process(e: IInterface): integer;
begin
  AddMessage('p');
  Result := 1;
end;

function Finalize: integer;
begin
  AddMessage('fin');
end;

end.
