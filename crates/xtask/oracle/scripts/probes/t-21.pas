unit P_t_21;

function Initialize: integer;
begin
  AddMessage('init');
end;

function Process(e: IInterface): integer;
begin
  Exit;
end;

function Finalize: integer;
begin
  AddMessage('fin');
end;

end.
