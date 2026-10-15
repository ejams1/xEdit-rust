unit P_t_24;

function Initialize: integer;
begin
  AddMessage('init');
end;

function Process(e: IInterface): integer;
begin end;

function Finalize: integer;
begin
  AddMessage('fin');
end;

end.
