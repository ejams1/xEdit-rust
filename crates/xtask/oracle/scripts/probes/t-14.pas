unit P_t_14;

function Initialize: integer;
begin
  AddMessage('init');
end;

function Process(e: IInterface): integer;
begin
  // a comment
end;

function Finalize: integer;
begin
  AddMessage('fin');
end;

end.
