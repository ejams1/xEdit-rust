{ parity probe: x-202 }
unit P_x_202;

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
  v := nil; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
