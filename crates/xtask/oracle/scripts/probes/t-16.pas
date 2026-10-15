unit P_t_16;

function Initialize: integer;
begin
  AddMessage('init');
end;

function Process(e: IInterface): integer;
begin
  RESULT:=0;
end;

function Finalize: integer;
begin
  AddMessage('fin');
end;

end.
