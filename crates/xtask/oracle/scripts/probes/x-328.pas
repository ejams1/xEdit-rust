{ parity probe: x-328 }
unit P_x_328;

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
  v := sl as TStringList; AddMessage(IntToStr(v.Count));
  AddMessage('after');
end;
end.
