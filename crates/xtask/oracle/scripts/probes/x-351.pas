{ parity probe: x-351 }
unit P_x_351;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  v := TList.Create; v.Delete(0);
  AddMessage('after');
end;
end.
