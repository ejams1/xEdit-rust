unit P_t_33;

function Initialize: integer;
begin
  AddMessage('init');
end;

function Process(e: IInterface): integer;
begin
  Result := 00;
end;

end.
