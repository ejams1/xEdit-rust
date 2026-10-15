unit P_t_38;

function Initialize: integer;
begin
  AddMessage('init');
end;

function Process(e: IInterface): integer;
begin
  Result := 0; // trailing comment
end;

end.
